use std::collections::HashMap;
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use agent_client_protocol::{AcpAgent, AcpAgentConfig};
#[cfg(test)]
use agent_client_protocol::{Client, ConnectTo};
use anyhow::{Context, bail};
use tokio::net::{UnixListener, UnixStream};
use tokio::task::JoinHandle;

use crate::config::{Config, ServerConfig};
use state::State;

pub mod acp;
mod ops;
mod server;
mod state;
mod state_file;
mod terminal;
#[cfg(test)]
mod tests;

/// Binds the socket, removing a stale one, then runs the daemon with the
/// state file and the servers from the config file. Without a config file, the
/// daemon waits for a server choice.
pub async fn start(path: &Path) -> anyhow::Result<()> {
    if path.exists() {
        if UnixStream::connect(path).await.is_ok() {
            bail!("a daemon is already running on {}", path.display());
        }
        std::fs::remove_file(path)
            .with_context(|| format!("removing stale socket {}", path.display()))?;
    }
    let listener =
        UnixListener::bind(path).with_context(|| format!("binding {}", path.display()))?;
    let state_file = ur_client::state_dir()
        .context("finding the state directory")?
        .join("state.json");
    let workspaces = state_file::read(&state_file)?;
    let state = Arc::new(Mutex::new(State::new(workspaces)));
    let control = Arc::new(ServerControl {
        state: state.clone(),
        tasks: Mutex::new(HashMap::new()),
        configuration: Mutex::new(Config {
            servers: Vec::new(),
        }),
    });
    match Config::read() {
        Ok(Some(config)) => {
            *control.configuration.lock().unwrap() = Config {
                servers: config.servers.clone(),
            };
            let ready: Vec<_> = config
                .servers
                .into_iter()
                .map(|server| control.start(server))
                .collect();
            futures::future::join_all(
                ready
                    .into_iter()
                    .map(|signal| async move { signal.notified().await }),
            )
            .await;
        }
        Ok(None) => {}
        Err(error) => state
            .lock()
            .unwrap()
            .set_config_error(Some(format!("{error:#}"))),
    }
    let terminals = Arc::new(terminal::Terminals::new(state.clone()));
    server::serve(listener, terminals, state, state_file, Some(control)).await
}

pub struct ServerControl {
    state: Arc<Mutex<State>>,
    tasks: Mutex<HashMap<String, JoinHandle<()>>>,
    configuration: Mutex<Config>,
}

impl ServerControl {
    fn validate_command(command: &str) -> anyhow::Result<()> {
        if !Path::new(&command).is_absolute() {
            anyhow::bail!("choose an absolute path to the server executable");
        }
        Ok(())
    }

    pub fn add(&self, name: String, command: String, args: Vec<String>) -> anyhow::Result<String> {
        Self::validate_command(&command)?;
        let mut configuration = self.configuration.lock().unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let server = ServerConfig {
            id: id.clone(),
            name,
            command,
            args,
        };
        let mut next = Config {
            servers: configuration.servers.clone(),
        };
        next.servers.push(server.clone());
        next.validate()?;
        next.write()?;
        *configuration = next;
        self.state.lock().unwrap().set_config_error(None);
        self.start(server);
        Ok(id)
    }

    pub fn update(
        &self,
        id: &str,
        name: String,
        command: String,
        args: Vec<String>,
    ) -> anyhow::Result<()> {
        Self::validate_command(&command)?;
        let mut configuration = self.configuration.lock().unwrap();
        let mut next = Config {
            servers: configuration.servers.clone(),
        };
        let server = next
            .servers
            .iter_mut()
            .find(|server| server.id == id)
            .ok_or_else(|| anyhow::anyhow!("no server {id}"))?;
        let restart = server.command != command || server.args != args;
        *server = ServerConfig {
            id: id.into(),
            name,
            command,
            args,
        };
        let server = server.clone();
        next.validate()?;
        next.write()?;
        *configuration = next;
        if restart {
            self.start(server);
        } else {
            self.state.lock().unwrap().configure_server(&server);
        }
        Ok(())
    }

    pub fn remove(&self, id: &str) -> anyhow::Result<()> {
        let mut configuration = self.configuration.lock().unwrap();
        if !configuration.servers.iter().any(|server| server.id == id) {
            anyhow::bail!("no server {id}");
        }
        let next = Config {
            servers: configuration
                .servers
                .iter()
                .filter(|server| server.id != id)
                .cloned()
                .collect(),
        };
        next.write()?;
        *configuration = next;
        if let Some(task) = self.tasks.lock().unwrap().remove(id) {
            task.abort();
        }
        self.state.lock().unwrap().remove_server(id);
        Ok(())
    }

    fn start(&self, config: ServerConfig) -> Arc<tokio::sync::Notify> {
        if let Some(task) = self.tasks.lock().unwrap().remove(&config.id) {
            task.abort();
            let mut state = self.state.lock().unwrap();
            if let Some(generation) = state.generation(&config.id) {
                state.server_exited(
                    &config.id,
                    generation,
                    "the server configuration changed".into(),
                );
            }
        }
        self.state.lock().unwrap().configure_server(&config);
        let command = config.command.clone();
        let id = config.id.clone();
        let launch = move || {
            AcpAgent::new(AcpAgentConfig::new(config.command.clone()).args(config.args.clone()))
        };
        let (task, ready) = acp::spawn_supervisor(id.clone(), command, launch, self.state.clone());
        self.tasks.lock().unwrap().insert(id, task);
        ready
    }
}

/// Reads the state file, starts the supervisor, then serves daemon clients
/// once the first `initialize` ends. An unreadable state file stops the
/// daemon. `server` holds the command that runs the server, for errors, and
/// the function that starts it, which the supervisor calls again after each
/// server exit. Daemon clients that connect meanwhile wait in the listen
/// backlog, so no request sees a server that is still starting. Without a
/// server, terminals, workspaces, and the transcripts held in memory still
/// work.
#[cfg(test)]
async fn run<S: ConnectTo<Client> + 'static>(
    listener: UnixListener,
    state_file: PathBuf,
    server: anyhow::Result<(String, impl Fn() -> S + Send + 'static)>,
) -> anyhow::Result<()> {
    let workspaces = state_file::read(&state_file)?;
    let state = Arc::new(Mutex::new(State::new(workspaces)));
    match server {
        Ok((command, launch)) => {
            let id = "test".to_string();
            state.lock().unwrap().configure_server(&ServerConfig {
                id: id.clone(),
                name: "Test".into(),
                command: command.clone(),
                args: Vec::new(),
            });
            acp::supervise(id, command, launch, state.clone()).await;
        }
        Err(error) => {
            let reason = format!("{error:#}");
            eprintln!("ur daemon: cannot start the server: {reason}");
            state.lock().unwrap().set_config_error(Some(reason));
        }
    }
    let terminals = Arc::new(terminal::Terminals::new(state.clone()));
    server::serve(listener, terminals, state, state_file, None).await
}
