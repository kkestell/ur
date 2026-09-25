//! The state file, `state.json` in `ur_client::state_dir()`.

use std::io;
use std::path::Path;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use ur_client::Workspace;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StateFile {
    workspaces: Vec<Workspace>,
}

/// The saved workspaces. A missing file means none.
pub fn read(path: &Path) -> anyhow::Result<Vec<Workspace>> {
    let json = match std::fs::read(path) {
        Ok(json) => json,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
    };
    let file: StateFile =
        serde_json::from_slice(&json).with_context(|| format!("parsing {}", path.display()))?;
    Ok(file.workspaces)
}

pub fn write(path: &Path, workspaces: &[Workspace]) -> anyhow::Result<()> {
    let file = StateFile {
        workspaces: workspaces.to_vec(),
    };
    std::fs::create_dir_all(path.parent().expect("the state file has a directory"))
        .and_then(|()| std::fs::write(path, serde_json::to_vec_pretty(&file)?))
        .with_context(|| format!("writing {}", path.display()))
}
