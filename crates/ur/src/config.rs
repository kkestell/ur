use std::io::ErrorKind;
use std::path::PathBuf;

use anyhow::{Context, anyhow};
use serde::{Deserialize, Serialize};

/// The config file, `$XDG_CONFIG_HOME/ur/config.json`, else
/// `~/.config/ur/config.json`.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub server: ServerConfig,
}

/// The server the daemon and the one-shot client launch.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
}

impl Config {
    /// Reads the config file, or `None` when there is none yet.
    pub fn read() -> anyhow::Result<Option<Config>> {
        let path = path()?;
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(error).with_context(|| format!("reading {}", path.display()));
            }
        };
        serde_json::from_str(&text)
            .map(Some)
            .with_context(|| format!("parsing {}", path.display()))
    }

    pub fn write(&self) -> anyhow::Result<()> {
        let path = path()?;
        std::fs::create_dir_all(path.parent().expect("config file has a parent"))?;
        let temporary = path.with_extension("json.tmp");
        std::fs::write(&temporary, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&temporary, &path).with_context(|| format!("saving {}", path.display()))?;
        Ok(())
    }
}

pub fn path() -> anyhow::Result<PathBuf> {
    let config = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(config) => PathBuf::from(config),
        None => std::env::home_dir()
            .ok_or_else(|| anyhow!("no home directory"))?
            .join(".config"),
    };
    Ok(config.join("ur/config.json"))
}
