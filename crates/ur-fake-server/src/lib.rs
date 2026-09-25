//! The fake server: a scripted ACP server for testing ur without a model
//! provider or Ox. Its binary is what the daemon launches in end-to-end tests,
//! and its library is the test agent in the daemon's tests.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    AgentCapabilities, AvailableCommand, AvailableCommandsUpdate, ContentBlock, ContentChunk,
    InitializeRequest, InitializeResponse, ListSessionsRequest, ListSessionsResponse,
    LoadSessionRequest, LoadSessionResponse, NewSessionRequest, NewSessionResponse,
    PermissionOption, PermissionOptionKind, PromptRequest, PromptResponse,
    RequestPermissionOutcome, RequestPermissionRequest, SessionCapabilities, SessionId,
    SessionInfo, SessionInfoUpdate, SessionListCapabilities, SessionNotification, SessionUpdate,
    StopReason, ToolCall, ToolCallContent, ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields,
    ToolKind,
};
use agent_client_protocol::{Agent, Client, ConnectTo, ConnectionTo, on_receive_request};
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;

/// Ends a running `hold` script, or the next one if none is running.
#[derive(Clone, Default)]
pub struct Hold(Arc<Notify>);

impl Hold {
    pub fn release(&self) {
        self.0.notify_one();
    }
}

/// The fake server's saved history: every session it created, with its `cwd`,
/// session title, and the updates to replay. Clones share it, so saved
/// sessions outlive one ACP connection.
#[derive(Clone)]
pub struct SavedHistory(Arc<Mutex<Saved>>);

#[derive(Serialize, Deserialize)]
struct Saved {
    /// The file the saved history is written to after every change, if any.
    #[serde(skip)]
    file: Option<PathBuf>,
    /// Whether the fake server advertises `loadSession` and `session/list`.
    advertised: bool,
    /// The number of sessions created so far, so IDs are never reused.
    created: u32,
    sessions: Vec<SavedSession>,
}

#[derive(Serialize, Deserialize)]
struct SavedSession {
    id: SessionId,
    cwd: PathBuf,
    title: Option<String>,
    /// Every update sent for the session, and a `user_message_chunk` for each
    /// prompt's text.
    updates: Vec<SessionUpdate>,
    /// Set by the `unloadable` script.
    unloadable: bool,
}

impl Default for SavedHistory {
    /// Saved history with `loadSession` and `session/list` advertised.
    fn default() -> SavedHistory {
        SavedHistory::new(true)
    }
}

impl SavedHistory {
    /// Saved history with neither `loadSession` nor `session/list`
    /// advertised. Both methods answer method not found.
    pub fn unadvertised() -> SavedHistory {
        SavedHistory::new(false)
    }

    /// Saved history, advertised, read from `file` when it exists and written
    /// to it after every change, so it outlives the fake server process.
    pub fn file(file: PathBuf) -> SavedHistory {
        let mut saved = match std::fs::read(&file) {
            Ok(json) => serde_json::from_slice(&json).expect("the saved history file is valid"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Saved {
                file: None,
                advertised: true,
                created: 0,
                sessions: Vec::new(),
            },
            Err(error) => panic!("reading {}: {error}", file.display()),
        };
        saved.file = Some(file);
        SavedHistory(Arc::new(Mutex::new(saved)))
    }

    fn new(advertised: bool) -> SavedHistory {
        SavedHistory(Arc::new(Mutex::new(Saved {
            file: None,
            advertised,
            created: 0,
            sessions: Vec::new(),
        })))
    }

    fn advertised(&self) -> bool {
        self.0.lock().unwrap().advertised
    }

    /// Runs `f` on the saved history, then writes it to its file, if any.
    fn edit<R>(&self, f: impl FnOnce(&mut Saved) -> R) -> R {
        let mut saved = self.0.lock().unwrap();
        let result = f(&mut saved);
        if let Some(file) = &saved.file {
            let json = serde_json::to_vec(&*saved).expect("the saved history serializes");
            std::fs::write(file, json).expect("the saved history file is writable");
        }
        result
    }

    fn with_session<R>(&self, id: &SessionId, f: impl FnOnce(&mut SavedSession) -> R) -> Option<R> {
        self.edit(|saved| {
            saved
                .sessions
                .iter_mut()
                .find(|session| session.id == *id)
                .map(f)
        })
    }

    fn save(&self, id: &SessionId, update: SessionUpdate) {
        self.with_session(id, |session| session.updates.push(update));
    }
}

/// The fake server. It advertises protocol version 1 and, unless `history` is
/// unadvertised, `loadSession` and `session/list`. It names sessions `fake-1`,
/// `fake-2`, and so on, and sends an `available_commands_update` right after
/// each `session/new` response. `session/list` returns one session per page.
/// `session/prompt` answers an error for a session not created or loaded
/// during this ACP connection, and otherwise runs the script its text names:
/// `hold`, `tool`, `tools`, `reject`, `fail`, `title`, `unloadable`,
/// `render`, or anything else for a reply.
pub fn fake_server(hold: Hold, history: SavedHistory) -> impl ConnectTo<Client> {
    // The sessions created or loaded during this ACP connection.
    let loaded = Arc::new(Mutex::new(HashSet::<SessionId>::new()));
    Agent
        .builder()
        .name("ur-fake-server")
        .on_receive_request(
            {
                let history = history.clone();
                async move |_: InitializeRequest, responder, _connection| {
                    let mut capabilities = AgentCapabilities::new();
                    if history.advertised() {
                        capabilities = capabilities.load_session(true).session_capabilities(
                            SessionCapabilities::new().list(SessionListCapabilities::new()),
                        );
                    }
                    responder.respond(
                        InitializeResponse::new(ProtocolVersion::V1)
                            .agent_capabilities(capabilities),
                    )
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            {
                let history = history.clone();
                let loaded = loaded.clone();
                async move |request: NewSessionRequest,
                            responder,
                            connection: ConnectionTo<Client>| {
                    let session = history.edit(|saved| {
                        saved.created += 1;
                        let session = SessionId::from(format!("fake-{}", saved.created));
                        saved.sessions.push(SavedSession {
                            id: session.clone(),
                            cwd: request.cwd,
                            title: None,
                            updates: Vec::new(),
                            unloadable: false,
                        });
                        session
                    });
                    loaded.lock().unwrap().insert(session.clone());
                    responder.respond(NewSessionResponse::new(session.clone()))?;
                    let command = AvailableCommand::new("tally", "count the tallies");
                    let update =
                        SessionUpdate::AvailableCommandsUpdate(AvailableCommandsUpdate::new(vec![
                            command,
                        ]));
                    history.save(&session, update.clone());
                    connection.send_notification(SessionNotification::new(session, update))
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            {
                let history = history.clone();
                async move |request: ListSessionsRequest, responder, _connection| {
                    if !history.advertised() {
                        return responder
                            .respond_with_error(agent_client_protocol::Error::method_not_found());
                    }
                    let saved = history.0.lock().unwrap();
                    let matching: Vec<_> = saved
                        .sessions
                        .iter()
                        .filter(|session| {
                            request.cwd.as_ref().is_none_or(|cwd| session.cwd == *cwd)
                        })
                        .collect();
                    let index = match request.cursor.as_deref().map(str::parse::<usize>) {
                        None => 0,
                        Some(Ok(index)) => index,
                        Some(Err(_)) => {
                            return responder.respond_with_error(
                                agent_client_protocol::Error::invalid_params(),
                            );
                        }
                    };
                    let page = matching
                        .get(index)
                        .map(|session| {
                            SessionInfo::new(session.id.clone(), session.cwd.clone())
                                .title(session.title.clone())
                        })
                        .into_iter()
                        .collect();
                    let next = (index + 1 < matching.len()).then(|| (index + 1).to_string());
                    responder.respond(ListSessionsResponse::new(page).next_cursor(next))
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            {
                let history = history.clone();
                let loaded = loaded.clone();
                async move |request: LoadSessionRequest,
                            responder,
                            connection: ConnectionTo<Client>| {
                    if !history.advertised() {
                        return responder
                            .respond_with_error(agent_client_protocol::Error::method_not_found());
                    }
                    let session = request.session_id;
                    let replay = history
                        .with_session(&session, |saved| (saved.unloadable, saved.updates.clone()));
                    let updates = match replay {
                        None => {
                            return responder
                                .respond_with_internal_error(format!("no session {session}"));
                        }
                        Some((true, _)) => {
                            return responder.respond_with_internal_error(
                                "the fake server cannot load this session",
                            );
                        }
                        Some((false, updates)) => updates,
                    };
                    for update in updates {
                        connection
                            .send_notification(SessionNotification::new(session.clone(), update))?;
                    }
                    loaded.lock().unwrap().insert(session);
                    responder.respond(LoadSessionResponse::new())
                }
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
                let session = request.session_id;
                if !loaded.lock().unwrap().contains(&session) {
                    return responder
                        .respond_with_internal_error(format!("session {session} is not loaded"));
                }
                // Saved for replay only, so live transcripts hold the daemon's
                // user prompt entry instead.
                history.save(
                    &session,
                    SessionUpdate::UserMessageChunk(ContentChunk::new(text.as_str().into())),
                );
                // Scripts wait for a permission answer or the hold, so they run
                // outside the dispatch loop.
                let hold = hold.clone();
                let history = history.clone();
                connection.spawn({
                    let connection = connection.clone();
                    async move {
                        let script = Script {
                            connection,
                            session,
                            history,
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
                            "title" => script.title()?,
                            "unloadable" => script.unloadable()?,
                            "render" => script.render()?,
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
    history: SavedHistory,
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

    /// Sends a tool call with content and asks permission for it, with the
    /// options `go` and `stop`. The request carries no content of its own.
    async fn ask(
        &self,
        tool_call_id: &str,
    ) -> agent_client_protocol::Result<RequestPermissionOutcome> {
        let tool_call = ToolCall::new(tool_call_id.to_string(), "count the tallies")
            .kind(ToolKind::Search)
            .raw_input(serde_json::json!({ "glob": "*.tally" }))
            .content(vec![ToolCallContent::from("every *.tally file")]);
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

    /// Sets the session title `tallies`.
    fn title(&self) -> agent_client_protocol::Result<StopReason> {
        self.history.with_session(&self.session, |saved| {
            saved.title = Some("tallies".to_string());
        });
        self.update(SessionUpdate::SessionInfoUpdate(
            SessionInfoUpdate::new().title("tallies".to_string()),
        ))?;
        Ok(StopReason::EndTurn)
    }

    /// Replies, then makes every later `session/load` of the session fail.
    fn unloadable(&self) -> agent_client_protocol::Result<StopReason> {
        self.history
            .with_session(&self.session, |saved| saved.unloadable = true);
        self.reply("unloadable")
    }

    /// Sends a thought, a completed `execute` tool call with its output, a
    /// completed `read` tool call with its content, and a Markdown message
    /// with bold text, a list, and inline code.
    fn render(&self) -> agent_client_protocol::Result<StopReason> {
        self.update(SessionUpdate::AgentThoughtChunk(ContentChunk::new(
            "weighing the tallies".into(),
        )))?;
        self.update(SessionUpdate::ToolCall(
            ToolCall::new("run-1", "ls *.tally")
                .kind(ToolKind::Execute)
                .status(ToolCallStatus::Completed)
                .content(vec![ToolCallContent::from("a.tally\nb.tally")]),
        ))?;
        self.update(SessionUpdate::ToolCall(
            ToolCall::new("read-1", "read a.tally")
                .kind(ToolKind::Read)
                .status(ToolCallStatus::Completed)
                .content(vec![ToolCallContent::from("one tally")]),
        ))?;
        self.message("**Two** tallies:\n\n- `a.tally`\n- `b.tally`\n")?;
        Ok(StopReason::EndTurn)
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

    /// Sends the update and saves it for replay.
    fn update(&self, update: SessionUpdate) -> agent_client_protocol::Result<()> {
        self.history.save(&self.session, update.clone());
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
