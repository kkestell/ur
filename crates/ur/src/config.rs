use std::path::PathBuf;

use anyhow::{Context, anyhow};
use serde::Deserialize;

/// The config file, `$XDG_CONFIG_HOME/ur/config.toml`, else
/// `~/.config/ur/config.toml`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub server: ServerConfig,
}

/// The server the daemon and the one-shot client launch.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
}

impl Config {
    pub fn read() -> anyhow::Result<Config> {
        let path = path()?;
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }
}

fn path() -> anyhow::Result<PathBuf> {
    let config = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(config) => PathBuf::from(config),
        None => std::env::home_dir()
            .ok_or_else(|| anyhow!("no home directory"))?
            .join(".config"),
    };
    Ok(config.join("ur/config.toml"))
}
