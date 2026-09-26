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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum Request {
    /// Starts a login shell in the workspace path.
    OpenTerminal { workspace: String },
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
    DetachTerminal { terminal: TerminalId },
    /// Sends `SIGHUP` to the terminal's shell. The terminal is removed, and
    /// `terminal_exited` sent, when the shell exits.
    CloseTerminal { terminal: TerminalId },
    /// Sends the watch snapshot, then every change to workspaces, sessions,
    /// and terminals. Watching again from the same socket connection replaces
    /// the earlier registration.
    Watch,
    /// Adds a workspace. `path` must be an absolute path to a directory, and is
    /// stored as is.
    AddWorkspace { name: String, path: PathBuf },
    /// Cancels the workspace's running prompts, then removes the workspace and
    /// its sessions, and stops its terminals.
    RemoveWorkspace { name: String },
    /// Creates a session with the workspace path as its `cwd`.
    NewSession { workspace: String },
    /// Deletes a session from the server, when it advertises `session/delete`.
    /// A running turn is cancelled first, and the response waits for
    /// `session/delete` to return.
    DeleteSession {
        #[ts(type = "string")]
        session: SessionId,
    },
    /// Sends the session snapshot, then every later transcript entry.
    /// Subscribing to a saved session loads it first, and the snapshot and the
    /// response wait for the load. Subscribing again from the same socket
    /// connection replaces the earlier subscription.
    Subscribe {
        #[ts(type = "string")]
        session: SessionId,
    },
    /// Replaces the sessions this socket connection focuses, and clears their
    /// unread flags.
    Focus {
        #[ts(type = "Array<string>")]
        sessions: Vec<SessionId>,
    },
    /// Sends `session/prompt` and answers once it is sent, or answers busy. A
    /// saved session is loaded first, and the prompt is sent when the load
    /// succeeds.
    Prompt {
        #[ts(type = "string")]
        session: SessionId,
        #[ts(type = "Array<import(\"@agentclientprotocol/sdk\").ContentBlock>")]
        content: Vec<ContentBlock>,
    },
    /// Sends `session/cancel` and answers every pending permission request of
    /// the session with `Cancelled`. Does nothing when no prompt is running.
    Cancel {
        #[ts(type = "string")]
        session: SessionId,
    },
    /// Answers a pending permission request. The first answer wins.
    AnswerPermission {
        #[ts(type = "string")]
        session: SessionId,
        request_id: u32,
        #[ts(type = "string")]
        option_id: PermissionOptionId,
    },
    /// Sends `session/set_config_option` and answers when the server responds.
    SetConfigOption {
        #[ts(type = "string")]
        session: SessionId,
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
        session: SessionId,
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
        /// The current ACP connection's capabilities, or `None` without one.
        #[ts(type = "import(\"@agentclientprotocol/sdk\").AgentCapabilities | null")]
        capabilities: Option<Box<AgentCapabilities>>,
    },
    /// A successful `initialize` started a new ACP connection with these
    /// capabilities.
    CapabilitiesChanged {
        #[ts(type = "import(\"@agentclientprotocol/sdk\").AgentCapabilities")]
        capabilities: Box<AgentCapabilities>,
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
        session: SessionId,
    },
    /// A subscribed session's transcript and config options, sent before its
    /// later entries. A load sends a fresh one, which replaces the earlier
    /// transcript.
    SessionSnapshot {
        #[ts(type = "string")]
        session: SessionId,
        transcript: Vec<Entry>,
        #[ts(type = "Array<import(\"@agentclientprotocol/sdk\").SessionConfigOption>")]
        config_options: Vec<SessionConfigOption>,
    },
    /// One transcript entry of a subscribed session, after its snapshot.
    Entry {
        #[ts(type = "string")]
        session: SessionId,
        entry: Entry,
    },
    /// A subscribed session's config options changed, after its snapshot.
    ConfigOptionsChanged {
        #[ts(type = "string")]
        session: SessionId,
        #[ts(type = "Array<import(\"@agentclientprotocol/sdk\").SessionConfigOption>")]
        config_options: Vec<SessionConfigOption>,
    },
    /// A subscribed session was removed with its workspace, or deleted.
    SessionRemoved {
        #[ts(type = "string")]
        session: SessionId,
    },
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
    #[ts(type = "string")]
    pub session: SessionId,
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
