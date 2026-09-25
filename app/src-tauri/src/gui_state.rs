use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ur_client::TerminalId;

/// The GUI state file, keyed by socket path.
#[derive(Default, Serialize, Deserialize)]
pub struct GuiState {
    selections: HashMap<String, Selection>,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Selection {
    Terminal(TerminalId),
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

    pub fn selection(&self, socket: &Path) -> Option<Selection> {
        self.selections.get(&key(socket)).copied()
    }

    pub fn select(&mut self, socket: &Path, selection: Selection) {
        self.selections.insert(key(socket), selection);
    }
}

fn key(socket: &Path) -> String {
    socket.display().to_string()
}

/// `$XDG_STATE_HOME/ur/gui.json`, else `~/.local/state/ur/gui.json`.
fn path() -> io::Result<PathBuf> {
    let state = match std::env::var_os("XDG_STATE_HOME") {
        Some(state) => PathBuf::from(state),
        None => std::env::home_dir()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no home directory"))?
            .join(".local/state"),
    };
    Ok(state.join("ur/gui.json"))
}
