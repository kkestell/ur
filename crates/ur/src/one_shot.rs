use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use agent_client_protocol::schema::v1::{
    CancelNotification, ContentBlock, NewSessionRequest, PromptRequest, RequestPermissionOutcome,
    RequestPermissionRequest, RequestPermissionResponse, SelectedPermissionOutcome,
    SessionNotification, StopReason,
};
use agent_client_protocol::{
    AcpAgent, AcpAgentConfig, Agent, Client, ConnectTo, ConnectionTo, Error,
    on_receive_notification, on_receive_request,
};
use serde::Serialize;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, BufReader};

use crate::config::Config;
use crate::daemon::acp;

/// Runs `agent-run` against the server in the config file, answering
/// permission requests from stdin.
pub async fn start(workspace: &Path, prompt: String) -> anyhow::Result<()> {
    let server = Config::read()?.server;
    let server = AcpAgent::new(AcpAgentConfig::new(server.command).args(server.args));
    let output = Arc::new(Mutex::new(std::io::stdout()));
    let answers = BufReader::new(tokio::io::stdin());
    let stop = run(server, workspace, prompt, answers, output.clone()).await?;
    print(&output, acp_name(stop))?;
    Ok(())
}

/// Sends one prompt in a new session, printing each line of output described
/// in the one-shot client plan, and returns the turn's stop reason.
pub async fn run<W: Write + Send + 'static>(
    server: impl ConnectTo<Client> + 'static,
    workspace: &Path,
    prompt: String,
    mut answers: impl AsyncBufRead + Unpin + Send + 'static,
    output: Arc<Mutex<W>>,
) -> anyhow::Result<StopReason> {
    let cwd = std::path::absolute(workspace)?;
    let stop = Client
        .builder()
        .on_receive_notification(
            {
                let output = output.clone();
                async move |notification: SessionNotification, _connection| {
                    print_json(&output, &notification.update)
                }
            },
            on_receive_notification!(),
        )
        .on_receive_request(
            {
                let output = output.clone();
                // Waiting for the answer here holds the SDK's dispatch loop,
                // which keeps the question the last thing printed. It waits
                // only on the answers input, so it cannot deadlock.
                async move |request: RequestPermissionRequest,
                            responder,
                            connection: ConnectionTo<Agent>| {
                    print_json(&output, &request)?;
                    for (number, option) in (1..).zip(&request.options) {
                        let kind = acp_name(option.kind);
                        print(&output, format!("{number}. {} ({kind})", option.name))?;
                    }
                    let outcome =
                        match read_answer(&mut answers, &output, request.options.len()).await? {
                            Some(index) => {
                                RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
                                    request.options[index].option_id.clone(),
                                ))
                            }
                            None => {
                                connection.send_notification(CancelNotification::new(
                                    request.session_id.clone(),
                                ))?;
                                RequestPermissionOutcome::Cancelled
                            }
                        };
                    responder.respond(RequestPermissionResponse::new(outcome))
                }
            },
            on_receive_request!(),
        )
        .connect_with(server, async |connection: ConnectionTo<Agent>| {
            Ok(prompt_once(connection, cwd, prompt, &output).await)
        })
        .await??;
    Ok(stop)
}

async fn prompt_once<W: Write>(
    connection: ConnectionTo<Agent>,
    cwd: PathBuf,
    prompt: String,
    output: &Mutex<W>,
) -> anyhow::Result<StopReason> {
    let initialize = acp::initialize(&connection).await?;
    print_json(output, &initialize)?;
    let session = connection
        .send_request(NewSessionRequest::new(cwd))
        .block_task()
        .await?;
    let response = connection
        .send_request(PromptRequest::new(
            session.session_id,
            vec![ContentBlock::from(prompt)],
        ))
        .block_task()
        .await?;
    Ok(response.stop_reason)
}

/// Reads answers until one is a permission option's number, and returns that
/// option's index, or `None` at the end of the answers input.
async fn read_answer<W: Write>(
    answers: &mut (impl AsyncBufRead + Unpin),
    output: &Mutex<W>,
    count: usize,
) -> agent_client_protocol::Result<Option<usize>> {
    let mut line = String::new();
    loop {
        line.clear();
        let read = answers
            .read_line(&mut line)
            .await
            .map_err(Error::into_internal_error)?;
        if read == 0 {
            return Ok(None);
        }
        match line.trim().parse::<usize>() {
            Ok(number) if (1..=count).contains(&number) => return Ok(Some(number - 1)),
            _ => print(output, format!("answer with a number from 1 to {count}"))?,
        }
    }
}

fn print_json<W: Write>(
    output: &Mutex<W>,
    value: &impl Serialize,
) -> agent_client_protocol::Result<()> {
    print(output, serde_json::to_string(value)?)
}

fn print<W: Write>(output: &Mutex<W>, line: String) -> agent_client_protocol::Result<()> {
    writeln!(output.lock().unwrap(), "{line}").map_err(Error::into_internal_error)
}

/// The ACP name of a unit enum value, such as `end_turn`.
fn acp_name(value: impl Serialize) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(name)) => name,
        other => panic!("not a unit enum value: {other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use agent_client_protocol::Channel;
    use agent_client_protocol::schema::ProtocolVersion;
    use agent_client_protocol::schema::v1::{
        ContentChunk, InitializeRequest, InitializeResponse, NewSessionResponse, PermissionOption,
        PermissionOptionKind, PromptResponse, SessionUpdate, ToolCall, ToolCallUpdate,
        ToolCallUpdateFields,
    };

    use super::*;

    const SESSION: &str = "test-session";
    const TOOL_CALL: &str = "test-call";

    /// What the test agent received from `run`.
    #[derive(Default)]
    struct Received {
        cwd: Option<PathBuf>,
        prompt: Option<String>,
        outcome: Option<RequestPermissionOutcome>,
        cancelled: bool,
    }

    struct Finished {
        stop: anyhow::Result<StopReason>,
        output: String,
        received: Received,
    }

    fn tool_call() -> ToolCall {
        ToolCall::new(TOOL_CALL, "list things")
    }

    fn permission_request() -> RequestPermissionRequest {
        RequestPermissionRequest::new(
            SESSION,
            ToolCallUpdate::new(TOOL_CALL, ToolCallUpdateFields::new()),
            vec![
                PermissionOption::new("go", "Go ahead", PermissionOptionKind::AllowOnce),
                PermissionOption::new("hold", "Hold off", PermissionOptionKind::RejectOnce),
            ],
        )
    }

    fn message(text: &str) -> SessionUpdate {
        SessionUpdate::AgentMessageChunk(ContentChunk::new(text.into()))
    }

    /// On `session/prompt`, sends a tool call, asks permission for it, and
    /// ends the turn with a message naming the chosen option, or with
    /// `cancelled`.
    fn test_agent(
        protocol_version: ProtocolVersion,
        received: Arc<Mutex<Received>>,
    ) -> impl ConnectTo<Client> {
        Agent
            .builder()
            .on_receive_request(
                async move |_: InitializeRequest, responder, _connection| {
                    responder.respond(InitializeResponse::new(protocol_version))
                },
                on_receive_request!(),
            )
            .on_receive_request(
                {
                    let received = received.clone();
                    async move |request: NewSessionRequest, responder, _connection| {
                        received.lock().unwrap().cwd = Some(request.cwd);
                        responder.respond(NewSessionResponse::new(SESSION))
                    }
                },
                on_receive_request!(),
            )
            .on_receive_request(
                {
                    let received = received.clone();
                    async move |request: PromptRequest,
                                responder,
                                connection: ConnectionTo<Client>| {
                        let [ContentBlock::Text(text)] = &request.prompt[..] else {
                            panic!("expected one text block: {:?}", request.prompt);
                        };
                        received.lock().unwrap().prompt = Some(text.text.clone());
                        connection.send_notification(SessionNotification::new(
                            SESSION,
                            SessionUpdate::ToolCall(tool_call()),
                        ))?;
                        // Asking from the dispatch loop would deadlock it.
                        let received = received.clone();
                        connection.spawn({
                            let connection = connection.clone();
                            async move {
                                let response = connection
                                    .send_request(permission_request())
                                    .block_task()
                                    .await?;
                                received.lock().unwrap().outcome = Some(response.outcome.clone());
                                let stop = match response.outcome {
                                    RequestPermissionOutcome::Selected(selected) => {
                                        connection.send_notification(SessionNotification::new(
                                            SESSION,
                                            message(&selected.option_id.to_string()),
                                        ))?;
                                        StopReason::EndTurn
                                    }
                                    _ => StopReason::Cancelled,
                                };
                                responder.respond(PromptResponse::new(stop))
                            }
                        })
                    }
                },
                on_receive_request!(),
            )
            .on_receive_notification(
                async move |_: CancelNotification, _connection| {
                    received.lock().unwrap().cancelled = true;
                    Ok(())
                },
                on_receive_notification!(),
            )
    }

    async fn run_test_agent(
        protocol_version: ProtocolVersion,
        workspace: &Path,
        answers: &'static [u8],
    ) -> Finished {
        let received = Arc::new(Mutex::new(Received::default()));
        let (client, agent) = Channel::duplex();
        tokio::spawn(test_agent(protocol_version, received.clone()).connect_to(agent));
        let output = Arc::new(Mutex::new(Vec::new()));
        let stop = run(
            client,
            workspace,
            "list the things".to_string(),
            answers,
            output.clone(),
        )
        .await;
        let output = String::from_utf8(output.lock().unwrap().clone()).unwrap();
        let received = std::mem::take(&mut *received.lock().unwrap());
        Finished {
            stop,
            output,
            received,
        }
    }

    fn json(value: &impl Serialize) -> String {
        serde_json::to_string(value).unwrap()
    }

    #[tokio::test]
    async fn runs_one_prompt_and_prints_each_update() {
        let workspace = Path::new("some/workspace");
        let finished = run_test_agent(ProtocolVersion::V1, workspace, b"1\n").await;

        let expected = [
            json(&InitializeResponse::new(ProtocolVersion::V1)),
            json(&SessionUpdate::ToolCall(tool_call())),
            json(&permission_request()),
            "1. Go ahead (allow_once)".to_string(),
            "2. Hold off (reject_once)".to_string(),
            json(&message("go")),
        ];
        assert_eq!(finished.output.lines().collect::<Vec<_>>(), expected);
        assert_eq!(finished.stop.unwrap(), StopReason::EndTurn);
        assert_eq!(
            finished.received.cwd,
            Some(std::path::absolute(workspace).unwrap())
        );
        assert_eq!(finished.received.prompt.as_deref(), Some("list the things"));
    }

    #[tokio::test]
    async fn answers_permission_from_input() {
        let selected = |id: &str| {
            RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(id.to_string()))
        };
        let cases: [(&str, &[u8], RequestPermissionOutcome, StopReason, usize); 4] = [
            ("1", b"1\n", selected("go"), StopReason::EndTurn, 0),
            ("2", b"2\n", selected("hold"), StopReason::EndTurn, 0),
            (
                "9 then 2",
                b"9\n2\n",
                selected("hold"),
                StopReason::EndTurn,
                1,
            ),
            (
                "end of input",
                b"",
                RequestPermissionOutcome::Cancelled,
                StopReason::Cancelled,
                0,
            ),
        ];
        for (name, answers, outcome, stop, retries) in cases {
            let finished = run_test_agent(ProtocolVersion::V1, Path::new("."), answers).await;

            assert_eq!(finished.stop.unwrap(), stop, "{name}: stop reason");
            let cancelled = outcome == RequestPermissionOutcome::Cancelled;
            assert_eq!(finished.received.outcome, Some(outcome), "{name}: outcome");
            assert_eq!(
                finished.received.cancelled, cancelled,
                "{name}: session/cancel sent"
            );
            assert_eq!(
                finished
                    .output
                    .matches("answer with a number from 1 to 2")
                    .count(),
                retries,
                "{name}: retry lines"
            );
        }
    }

    #[tokio::test]
    async fn rejects_an_unsupported_protocol_version() {
        let finished = run_test_agent(ProtocolVersion::from(2), Path::new("."), b"").await;

        let error = finished.stop.unwrap_err().to_string();
        assert!(error.contains("ACP version 2"), "{error}");
    }
}
