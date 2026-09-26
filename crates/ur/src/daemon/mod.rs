use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use agent_client_protocol::{AcpAgent, AcpAgentConfig};
#[cfg(test)]
use agent_client_protocol::{Client, ConnectTo};
use anyhow::{Context, bail};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Notify;
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
/// state file and the server from the config file.
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
        task: Mutex::new(None),
        configuration: Mutex::new(()),
    });
    match Config::read() {
        Ok(config) => {
            let ready = control.start(config.server);
            ready.notified().await;
        }
        Err(error) => state.lock().unwrap().set_server(Err(format!("{error:#}"))),
    }
    let terminals = Arc::new(terminal::Terminals::new(state.clone()));
    server::serve(listener, terminals, state, state_file, Some(control)).await
}

pub struct ServerControl {
    state: Arc<Mutex<State>>,
    task: Mutex<Option<JoinHandle<()>>>,
    configuration: Mutex<()>,
}

impl ServerControl {
    pub fn configure(&self, command: String, args: Vec<String>) -> anyhow::Result<()> {
        let _configuration = self.configuration.lock().unwrap();
        if !Path::new(&command).is_absolute() {
            anyhow::bail!("choose an absolute path to the server executable");
        }
        let config = ServerConfig { command, args };
        (Config {
            server: config.clone(),
        })
        .write()?;
        self.start(config);
        Ok(())
    }

    fn start(&self, config: ServerConfig) -> Arc<Notify> {
        if let Some(task) = self.task.lock().unwrap().take() {
            task.abort();
            self.state
                .lock()
                .unwrap()
                .server_exited("the server configuration changed".into());
        }
        self.state
            .lock()
            .unwrap()
            .configure_server(config.command.clone(), config.args.clone());
        let command = config.command.clone();
        let launch = move || {
            AcpAgent::new(AcpAgentConfig::new(config.command.clone()).args(config.args.clone()))
        };
        let (task, ready) = acp::spawn_supervisor(command, launch, self.state.clone());
        *self.task.lock().unwrap() = Some(task);
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
            acp::supervise(command, launch, state.clone()).await;
        }
        Err(error) => {
            let reason = format!("{error:#}");
            eprintln!("ur daemon: cannot start the server: {reason}");
            state.lock().unwrap().set_server(Err(reason));
        }
    }
    let terminals = Arc::new(terminal::Terminals::new(state.clone()));
    server::serve(listener, terminals, state, state_file, None).await
}
