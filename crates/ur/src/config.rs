use std::collections::HashSet;
use std::io::ErrorKind;
use std::path::PathBuf;

use anyhow::{Context, anyhow};
use serde::{Deserialize, Serialize};

/// The config file, `$XDG_CONFIG_HOME/ur/config.json`, else
/// `~/.config/ur/config.json`.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub servers: Vec<ServerConfig>,
}

/// The server the daemon and the one-shot client launch.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    pub id: String,
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
}

impl Config {
    pub fn validate(&self) -> anyhow::Result<()> {
        let mut ids = HashSet::new();
        let mut names = HashSet::new();
        for server in &self.servers {
            if server.id.is_empty() || !ids.insert(&server.id) {
                anyhow::bail!("server IDs must be nonempty and unique: {}", server.id);
            }
            if server.name.trim().is_empty() || !names.insert(&server.name) {
                anyhow::bail!("server names must be nonempty and unique: {}", server.name);
            }
        }
        Ok(())
    }

    pub fn select(&self, name: Option<&str>) -> anyhow::Result<&ServerConfig> {
        match name {
            Some(name) => self
                .servers
                .iter()
                .find(|server| server.name == name)
                .ok_or_else(|| anyhow!("no server named {name}; available: {}", self.names())),
            None if self.servers.len() == 1 => Ok(&self.servers[0]),
            None => anyhow::bail!("choose a server with --server; available: {}", self.names()),
        }
    }

    fn names(&self) -> String {
        if self.servers.is_empty() {
            "none".into()
        } else {
            self.servers
                .iter()
                .map(|server| server.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        }
    }

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
        let config: Config =
            serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        config
            .validate()
            .with_context(|| format!("validating {}", path.display()))?;
        Ok(Some(config))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn config(names: &[&str]) -> Config {
        Config {
            servers: names
                .iter()
                .enumerate()
                .map(|(index, name)| ServerConfig {
                    id: format!("id-{index}"),
                    name: (*name).into(),
                    command: "/bin/echo".into(),
                    args: vec![],
                })
                .collect(),
        }
    }

    #[test]
    fn ordered_servers_round_trip_with_stable_ids() {
        let mut config = config(&["Alpha", "Beta"]);
        config.servers[0].name = "Renamed".into();
        let round_trip: Config =
            serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap();
        round_trip.validate().unwrap();
        assert_eq!(
            round_trip
                .servers
                .iter()
                .map(|server| (&*server.id, &*server.name))
                .collect::<Vec<_>>(),
            vec![("id-0", "Renamed"), ("id-1", "Beta")]
        );
    }

    #[test]
    fn duplicate_ids_and_names_are_rejected() {
        let mut config = config(&["Alpha", "Beta"]);
        config.servers[1].id = config.servers[0].id.clone();
        assert!(config.validate().unwrap_err().to_string().contains("IDs"));
        config.servers[1].id = "id-1".into();
        config.servers[1].name = "Alpha".into();
        assert!(config.validate().unwrap_err().to_string().contains("names"));
    }

    #[test]
    fn one_shot_selection_requires_a_name_when_ambiguous() {
        let zero = config(&[]);
        assert!(zero.select(None).is_err());
        let one = config(&["Alpha"]);
        assert_eq!(one.select(None).unwrap().id, "id-0");
        let several = config(&["Alpha", "Beta"]);
        assert!(
            several
                .select(None)
                .unwrap_err()
                .to_string()
                .contains("Alpha, Beta")
        );
        assert_eq!(several.select(Some("Beta")).unwrap().id, "id-1");
    }
}
