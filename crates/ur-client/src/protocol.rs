use std::path::PathBuf;

use agent_client_protocol_schema::v1::{ContentBlock, SessionId, SessionUpdate};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Assigned by the daemon, counting from 1. A `u32` so the TypeScript binding
/// is `number`, not `bigint`.
pub type TerminalId = u32;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum Request {
    OpenTerminal,
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
    /// Creates a session with `path` as its `cwd`, passed unchanged.
    NewSession {
        path: PathBuf,
    },
    /// Sends the session snapshot, then every later transcript entry.
    /// Subscribing again from the same socket connection replaces the earlier
    /// subscription.
    Subscribe {
        #[ts(type = "string")]
        session: SessionId,
    },
    /// Sends `session/prompt` and answers once it is sent, or answers busy.
    Prompt {
        #[ts(type = "string")]
        session: SessionId,
        #[ts(type = "Array<import(\"@agentclientprotocol/sdk\").ContentBlock>")]
        content: Vec<ContentBlock>,
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
    /// The shell of an attached terminal exited, and the daemon removed the
    /// terminal.
    TerminalExited { terminal: TerminalId },
    /// A subscribed session's transcript, sent before its later entries.
    SessionSnapshot {
        #[ts(type = "string")]
        session: SessionId,
        transcript: Vec<Entry>,
    },
    /// One transcript entry of a subscribed session, after its snapshot.
    Entry {
        #[ts(type = "string")]
        session: SessionId,
        entry: Entry,
    },
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
