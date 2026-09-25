use std::path::PathBuf;

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
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum Response {
    Opened { terminal: TerminalId },
    Done,
    Error { message: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum Event {
    /// The shell of an attached terminal exited, and the daemon removed the
    /// terminal.
    TerminalExited { terminal: TerminalId },
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
