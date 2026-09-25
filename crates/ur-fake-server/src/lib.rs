//! The fake server: a scripted ACP server for testing ur without a model
//! provider or Ox. Its binary is what the daemon launches in end-to-end tests,
//! and its library is the test agent in the daemon's tests.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    AvailableCommand, AvailableCommandsUpdate, ContentBlock, ContentChunk, InitializeRequest,
    InitializeResponse, NewSessionRequest, NewSessionResponse, PermissionOption,
    PermissionOptionKind, PromptRequest, PromptResponse, RequestPermissionOutcome,
    RequestPermissionRequest, SessionId, SessionNotification, SessionUpdate, StopReason, ToolCall,
    ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields, ToolKind,
};
use agent_client_protocol::{Agent, Client, ConnectTo, ConnectionTo, on_receive_request};
use tokio::sync::Notify;

/// Ends a running `hold` script, or the next one if none is running.
#[derive(Clone, Default)]
pub struct Hold(Arc<Notify>);

impl Hold {
    pub fn release(&self) {
        self.0.notify_one();
    }
}

/// The fake server. It advertises protocol version 1 and no optional
/// capabilities, names sessions `fake-1`, `fake-2`, and so on, and sends an
/// `available_commands_update` right after each `session/new` response.
/// `session/prompt` runs the script its text names: `hold`, `tool`, `tools`,
/// `reject`, `fail`, or anything else for a reply.
pub fn fake_server(hold: Hold) -> impl ConnectTo<Client> {
    let sessions = Arc::new(AtomicU32::new(0));
    Agent
        .builder()
        .name("ur-fake-server")
        .on_receive_request(
            async move |_: InitializeRequest, responder, _connection| {
                responder.respond(InitializeResponse::new(ProtocolVersion::V1))
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |_: NewSessionRequest, responder, connection: ConnectionTo<Client>| {
                let number = sessions.fetch_add(1, Ordering::Relaxed) + 1;
                let session = SessionId::from(format!("fake-{number}"));
                responder.respond(NewSessionResponse::new(session.clone()))?;
                let command = AvailableCommand::new("tally", "count the tallies");
                connection.send_notification(SessionNotification::new(
                    session,
                    SessionUpdate::AvailableCommandsUpdate(AvailableCommandsUpdate::new(vec![
                        command,
                    ])),
                ))
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |request: PromptRequest, responder, connection: ConnectionTo<Client>| {
                let text = match &request.prompt[..] {
                    [ContentBlock::Text(text)] => text.text.clone(),
                    other => {
                        return responder.respond_with_internal_error(format!(
                            "expected one text block: {other:?}"
                        ));
                    }
                };
                // Scripts wait for a permission answer or the hold, so they run
                // outside the dispatch loop.
                let hold = hold.clone();
                connection.spawn({
                    let connection = connection.clone();
                    async move {
                        let script = Script {
                            connection,
                            session: request.session_id,
                        };
                        let stop = match text.as_str() {
                            "hold" => script.hold(&hold).await?,
                            "tool" => script.tool().await?,
                            "tools" => script.tools().await?,
                            "reject" => {
                                return responder.respond_with_internal_error(
                                    "the fake server rejects this prompt",
                                );
                            }
                            "fail" => {
                                script.message("failing")?;
                                return responder
                                    .respond_with_internal_error("the fake server failed");
                            }
                            _ => script.reply(&text)?,
                        };
                        responder.respond(PromptResponse::new(stop))
                    }
                })
            },
            on_receive_request!(),
        )
}

/// One prompt's script.
struct Script {
    connection: ConnectionTo<Client>,
    session: SessionId,
}

impl Script {
    async fn hold(&self, hold: &Hold) -> agent_client_protocol::Result<StopReason> {
        self.message("holding")?;
        hold.0.notified().await;
        self.message("released")?;
        Ok(StopReason::EndTurn)
    }

    /// Asks permission for one tool call. A selected option completes the
    /// tool call, and an error fails it.
    async fn tool(&self) -> agent_client_protocol::Result<StopReason> {
        let (status, message) = match self.ask("tally-1").await {
            Ok(RequestPermissionOutcome::Selected(selected)) => (
                ToolCallStatus::Completed,
                format!("selected {}", selected.option_id),
            ),
            Ok(RequestPermissionOutcome::Cancelled) => return Ok(StopReason::Cancelled),
            other => (ToolCallStatus::Failed, outcome(&other)),
        };
        self.update(SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
            "tally-1",
            ToolCallUpdateFields::new().status(status),
        )))?;
        self.message(&message)?;
        Ok(StopReason::EndTurn)
    }

    /// Asks permission for two tool calls at once. If either is cancelled, it
    /// asks for a third, which a well-behaved server would not, so tests can
    /// check that the daemon answers it `Cancelled`. Then it sends one message
    /// with each outcome, such as `tally-1: go, tally-2: cancelled`.
    async fn tools(&self) -> agent_client_protocol::Result<StopReason> {
        let (first, second) = tokio::join!(self.ask("tally-1"), self.ask("tally-2"));
        let mut outcomes = vec![("tally-1", first), ("tally-2", second)];
        let cancelled = |outcomes: &[(&str, _)]| {
            outcomes
                .iter()
                .any(|(_, outcome)| matches!(outcome, Ok(RequestPermissionOutcome::Cancelled)))
        };
        if cancelled(&outcomes) {
            outcomes.push(("tally-3", self.ask("tally-3").await));
        }
        let text: Vec<_> = outcomes
            .iter()
            .map(|(id, result)| format!("{id}: {}", outcome(result)))
            .collect();
        self.message(&text.join(", "))?;
        Ok(if cancelled(&outcomes) {
            StopReason::Cancelled
        } else {
            StopReason::EndTurn
        })
    }

    /// Sends a tool call and asks permission for it, with the options `go` and
    /// `stop`.
    async fn ask(
        &self,
        tool_call_id: &str,
    ) -> agent_client_protocol::Result<RequestPermissionOutcome> {
        let tool_call = ToolCall::new(tool_call_id.to_string(), "count the tallies")
            .kind(ToolKind::Search)
            .raw_input(serde_json::json!({ "glob": "*.tally" }));
        self.update(SessionUpdate::ToolCall(tool_call))?;
        let request = RequestPermissionRequest::new(
            self.session.clone(),
            ToolCallUpdate::new(tool_call_id.to_string(), ToolCallUpdateFields::new()),
            vec![
                PermissionOption::new("go", "Go ahead", PermissionOptionKind::AllowOnce),
                PermissionOption::new("stop", "Hold off", PermissionOptionKind::RejectOnce),
            ],
        );
        let response = self.connection.send_request(request).block_task().await?;
        Ok(response.outcome)
    }

    /// Replies `you said: <text>`, one agent message chunk per word.
    fn reply(&self, text: &str) -> agent_client_protocol::Result<StopReason> {
        for word in format!("you said: {text}").split_inclusive(' ') {
            self.message(word)?;
        }
        Ok(StopReason::EndTurn)
    }

    fn message(&self, text: &str) -> agent_client_protocol::Result<()> {
        self.update(SessionUpdate::AgentMessageChunk(ContentChunk::new(
            text.into(),
        )))
    }

    fn update(&self, update: SessionUpdate) -> agent_client_protocol::Result<()> {
        self.connection
            .send_notification(SessionNotification::new(self.session.clone(), update))
    }
}

/// A permission outcome as the scripts' messages show it: the selected option
/// ID, `cancelled`, or the error.
fn outcome(result: &agent_client_protocol::Result<RequestPermissionOutcome>) -> String {
    match result {
        Ok(RequestPermissionOutcome::Selected(selected)) => selected.option_id.to_string(),
        Ok(RequestPermissionOutcome::Cancelled) => "cancelled".to_string(),
        Ok(other) => format!("unknown outcome {other:?}"),
        Err(error) => format!("permission failed: {error}"),
    }
}
