use std::fmt;
use std::io;
use std::path::PathBuf;

use agent_client_protocol_schema::v1::{
    AgentCapabilities, ContentBlock, PermissionOptionId, RequestPermissionRequest, SessionConfigId,
    SessionConfigOption, SessionConfigOptionValue, SessionId, SessionUpdate, StopReason,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Assigned by the daemon, counting from 1. A `u32` so the TypeScript binding
/// is `number`, not `bigint`.
pub type TerminalId = u32;

/// Stable daemon reference to an ACP session owned by one configured server.
#[derive(Clone, Debug, Hash, Eq, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, as = "String")]
pub struct SessionKey(pub String);

impl SessionKey {
    pub fn new(server: &str, session: &SessionId) -> Self {
        Self(serde_json::to_string(&(server, &*session.0)).expect("session key serializes"))
    }
}

impl fmt::Display for SessionKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<String> for SessionKey {
    fn from(value: String) -> Self {
        Self(value)
    }
}

#[cfg(test)]
mod session_key_tests {
    use super::*;

    #[test]
    fn session_keys_separate_servers_and_escape_acp_ids() {
        let id = SessionId::new("a, [\\\"quoted\\\"]");
        let one = SessionKey::new("one", &id);
        let two = SessionKey::new("two", &id);
        assert_ne!(one, two);
        assert_eq!(one, SessionKey::new("one", &id));
        assert_eq!(
            serde_json::from_str::<(String, String)>(&one.0).unwrap(),
            ("one".into(), "a, [\\\"quoted\\\"]".into())
        );
        assert_eq!(
            serde_json::to_string(&one).unwrap(),
            serde_json::to_string(&one.0).unwrap()
        );
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum Request {
    /// Starts a login shell in the workspace path.
    OpenTerminal {
        workspace: String,
    },
    AttachTerminal {
        terminal: TerminalId,
        rows: u16,
        cols: u16,
    },
    TerminalResize {
        terminal: TerminalId,
        rows: u16,
        cols: u16,
    },
    /// Stops sending the terminal's output to this socket connection. The
    /// terminal keeps running.
    DetachTerminal {
        terminal: TerminalId,
    },
    /// Sends `SIGHUP` to the terminal's shell. The terminal is removed, and
    /// `terminal_exited` sent, when the shell exits.
    CloseTerminal {
        terminal: TerminalId,
    },
    /// Sends the watch snapshot, then every change to workspaces, sessions,
    /// and terminals. Watching again from the same socket connection replaces
    /// the earlier registration.
    Watch,
    AddServer {
        name: String,
        command: String,
        args: Vec<String>,
    },
    UpdateServer {
        server: String,
        name: String,
        command: String,
        args: Vec<String>,
    },
    RemoveServer {
        server: String,
    },
    /// Adds a workspace. `path` must be an absolute path to a directory, and is
    /// stored as is.
    AddWorkspace {
        name: String,
        path: PathBuf,
    },
    /// Cancels the workspace's running prompts, then removes the workspace and
    /// its sessions, and stops its terminals.
    RemoveWorkspace {
        name: String,
    },
    /// Creates a session with the workspace path as its `cwd`.
    NewSession {
        server: String,
        workspace: String,
    },
    /// Deletes a session from the server, when it advertises `session/delete`.
    /// A running turn is cancelled first, and the response waits for
    /// `session/delete` to return.
    DeleteSession {
        #[ts(type = "string")]
        session: SessionKey,
    },
    /// Sends the session snapshot, then every later transcript entry.
    /// Subscribing to a saved session loads it first, and the snapshot and the
    /// response wait for the load. Subscribing again from the same socket
    /// connection replaces the earlier subscription.
    Subscribe {
        #[ts(type = "string")]
        session: SessionKey,
    },
    /// Replaces the sessions this socket connection focuses, and clears their
    /// unread flags.
    Focus {
        #[ts(type = "Array<string>")]
        sessions: Vec<SessionKey>,
    },
    /// Sends `session/prompt` and answers once it is sent, or answers busy. A
    /// saved session is loaded first, and the prompt is sent when the load
    /// succeeds.
    Prompt {
        #[ts(type = "string")]
        session: SessionKey,
        #[ts(type = "Array<import(\"@agentclientprotocol/sdk\").ContentBlock>")]
        content: Vec<ContentBlock>,
    },
    /// Sends `session/cancel` and answers every pending permission request of
    /// the session with `Cancelled`. Does nothing when no prompt is running.
    Cancel {
        #[ts(type = "string")]
        session: SessionKey,
    },
    /// Answers a pending permission request. The first answer wins.
    AnswerPermission {
        #[ts(type = "string")]
        session: SessionKey,
        request_id: u32,
        #[ts(type = "string")]
        option_id: PermissionOptionId,
    },
    /// Sends `session/set_config_option` and answers when the server responds.
    SetConfigOption {
        #[ts(type = "string")]
        session: SessionKey,
        #[ts(type = "string")]
        config_id: SessionConfigId,
        #[ts(type = "{ type: \"boolean\"; value: boolean } | { value: string }")]
        value: SessionConfigOptionValue,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum Response {
    Opened {
        terminal: TerminalId,
    },
    SessionCreated {
        #[ts(type = "string")]
        session: SessionKey,
    },
    ServerAdded {
        server: String,
    },
    Done,
    /// The session's operation guard is held, and nothing changed.
    Busy,
    Error {
        message: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum Event {
    /// A terminal's shell exited, and the daemon removed the terminal. Sent
    /// once to each watching or attached socket connection, after all of the
    /// terminal's output.
    TerminalExited {
        terminal: TerminalId,
    },
    /// Every workspace in the order it was added, every session in discovery
    /// order, and every terminal in the order it was opened, sent before later
    /// changes.
    WatchSnapshot {
        workspaces: Vec<Workspace>,
        sessions: Vec<SessionSummary>,
        terminals: Vec<TerminalSummary>,
        servers: Vec<ServerState>,
        config_error: Option<String>,
    },
    ServersChanged {
        servers: Vec<ServerState>,
        config_error: Option<String>,
    },
    WorkspaceAdded {
        workspace: Workspace,
    },
    /// The workspace, its sessions, and its terminals are gone.
    WorkspaceRemoved {
        name: String,
    },
    /// A session was added, or its status, unread flag, session title, or last
    /// activity changed.
    SessionChanged {
        summary: SessionSummary,
    },
    /// A terminal was opened, or its terminal title changed.
    TerminalChanged {
        summary: TerminalSummary,
    },
    /// The session was deleted and is gone from the sidebar.
    SessionDeleted {
        #[ts(type = "string")]
        session: SessionKey,
    },
    /// A subscribed session's transcript and config options, sent before its
    /// later entries. A load sends a fresh one, which replaces the earlier
    /// transcript.
    SessionSnapshot {
        #[ts(type = "string")]
        session: SessionKey,
        transcript: Vec<Entry>,
        #[ts(type = "Array<import(\"@agentclientprotocol/sdk\").SessionConfigOption>")]
        config_options: Vec<SessionConfigOption>,
    },
    /// One transcript entry of a subscribed session, after its snapshot.
    Entry {
        #[ts(type = "string")]
        session: SessionKey,
        entry: Entry,
    },
    /// A subscribed session's config options changed, after its snapshot.
    ConfigOptionsChanged {
        #[ts(type = "string")]
        session: SessionKey,
        #[ts(type = "Array<import(\"@agentclientprotocol/sdk\").SessionConfigOption>")]
        config_options: Vec<SessionConfigOption>,
    },
    /// A subscribed session was removed with its workspace, or deleted.
    SessionRemoved {
        #[ts(type = "string")]
        session: SessionKey,
    },
}

/// One configured server and its current connection state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ServerState {
    pub id: String,
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub connected: bool,
    pub error: Option<String>,
    #[ts(type = "import(\"@agentclientprotocol/sdk\").AgentCapabilities | null")]
    pub capabilities: Option<Box<AgentCapabilities>>,
}

/// A workspace, as the wire protocol and the state file hold it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Workspace {
    pub name: String,
    pub path: PathBuf,
}

/// One session as watch shows it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SessionSummary {
    pub server: String,
    #[ts(type = "string")]
    pub session: SessionKey,
    pub workspace: String,
    pub status: Status,
    pub unread: bool,
    /// The session title, from `session/list` or `session_info_update`.
    pub title: Option<String>,
    /// The last activity, from `session/list` or `session_info_update`.
    pub updated_at: Option<String>,
}

/// One terminal as watch shows it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TerminalSummary {
    pub terminal: TerminalId,
    pub workspace: String,
    pub title: String,
}

/// The session status.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum Status {
    Idle {
        #[ts(type = "import(\"@agentclientprotocol/sdk\").StopReason | null")]
        last_stop: Option<StopReason>,
    },
    Working,
    /// Every pending permission request of the session, oldest first.
    NeedsPermission {
        requests: Vec<PendingPermission>,
    },
    Failed {
        message: String,
    },
}

/// A pending permission request.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PendingPermission {
    /// Assigned by the daemon, counting from 1 across all sessions. The
    /// JSON-RPC ID can be a string, a number, or null, and a `u32` keeps the
    /// TypeScript binding `number`.
    pub request_id: u32,
    #[ts(type = "import(\"@agentclientprotocol/sdk\").RequestPermissionRequest")]
    pub request: RequestPermissionRequest,
}

/// One transcript entry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum Entry {
    /// An ACP update entry: a `SessionUpdate` the daemon received.
    Update {
        #[ts(type = "import(\"@agentclientprotocol/sdk\").SessionUpdate")]
        update: Box<SessionUpdate>,
    },
    /// A user prompt entry: the content of a `session/prompt` the daemon sent.
    UserPrompt {
        #[ts(type = "Array<import(\"@agentclientprotocol/sdk\").ContentBlock>")]
        content: Vec<ContentBlock>,
    },
    /// A turn error entry: the message of the JSON-RPC error `session/prompt`
    /// returned, or why the server exited during the turn.
    TurnError { message: String },
}

/// The `JSON` frame payload from a daemon client.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClientMessage {
    pub id: u64,
    pub request: Request,
}

/// The `JSON` frame payload from the daemon.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DaemonMessage {
    Response { id: u64, response: Response },
    Event { event: Event },
}

/// `$UR_SOCKET`, else `$TMPDIR/ur.sock`.
pub fn socket_path() -> PathBuf {
    match std::env::var_os("UR_SOCKET") {
        Some(path) => path.into(),
        None => std::env::temp_dir().join("ur.sock"),
    }
}

/// `$XDG_STATE_HOME/ur`, else `~/.local/state/ur`. It holds the state file and
/// the GUI state file.
pub fn state_dir() -> io::Result<PathBuf> {
    let state = match std::env::var_os("XDG_STATE_HOME") {
        Some(state) => PathBuf::from(state),
        None => std::env::home_dir()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no home directory"))?
            .join(".local/state"),
    };
    Ok(state.join("ur"))
}
