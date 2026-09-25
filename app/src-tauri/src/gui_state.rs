use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use ur_client::TerminalId;

/// The GUI state file, keyed by socket path.
#[derive(Default, Serialize, Deserialize)]
pub struct GuiState {
    saved: HashMap<String, Saved>,
}

/// One socket path's record: the GUI's one terminal, until Workspace terminal
/// controls list terminals under their workspaces, and the selection.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Saved {
    pub terminal: Option<TerminalId>,
    pub selection: Option<Selection>,
}

/// The session or terminal chosen in the sidebar.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum Selection {
    Terminal,
    Session { session: String },
}

/// The payload of the `connection` event and command.
#[derive(Clone, Debug, Serialize, TS)]
#[ts(export)]
pub struct Connection {
    pub connected: bool,
    pub socket: String,
}

impl GuiState {
    pub fn load() -> io::Result<GuiState> {
        match std::fs::read(path()?) {
            Ok(json) => serde_json::from_slice(&json).map_err(io::Error::from),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(GuiState::default()),
            Err(error) => Err(error),
        }
    }

    pub fn save(&self) -> io::Result<()> {
        let path = path()?;
        std::fs::create_dir_all(path.parent().expect("the GUI state file has a directory"))?;
        std::fs::write(path, serde_json::to_vec_pretty(self)?)
    }

    pub fn saved(&self, socket: &Path) -> Saved {
        self.saved.get(&key(socket)).cloned().unwrap_or_default()
    }

    pub fn saved_mut(&mut self, socket: &Path) -> &mut Saved {
        self.saved.entry(key(socket)).or_default()
    }
}

fn key(socket: &Path) -> String {
    socket.display().to_string()
}

/// `gui.json` in `ur_client::state_dir()`.
fn path() -> io::Result<PathBuf> {
    Ok(ur_client::state_dir()?.join("gui.json"))
}
