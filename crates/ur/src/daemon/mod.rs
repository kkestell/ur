use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use agent_client_protocol::{AcpAgent, AcpAgentConfig, Client, ConnectTo};
use anyhow::{Context, bail};
use tokio::net::{UnixListener, UnixStream};

use crate::config::Config;
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
    let server = Config::read().map(|config| {
        let command = config.server.command;
        let args = config.server.args;
        let launch = {
            let command = command.clone();
            move || AcpAgent::new(AcpAgentConfig::new(command.clone()).args(args.clone()))
        };
        (command, launch)
    });
    let state_file = ur_client::state_dir()
        .context("finding the state directory")?
        .join("state.json");
    run(listener, state_file, server).await
}

/// Reads the state file, starts the supervisor, then serves daemon clients
/// once the first `initialize` ends. An unreadable state file stops the
/// daemon. `server` holds the command that runs the server, for errors, and
/// the function that starts it, which the supervisor calls again after each
/// server exit. Daemon clients that connect meanwhile wait in the listen
/// backlog, so no request sees a server that is still starting. Without a
/// server, terminals, workspaces, and the transcripts held in memory still
/// work.
async fn run<S: ConnectTo<Client> + 'static>(
    listener: UnixListener,
    state_file: PathBuf,
    server: anyhow::Result<(String, impl Fn() -> S + Send + 'static)>,
) -> anyhow::Result<()> {
    let workspaces = state_file::read(&state_file)?;
    let state = Arc::new(Mutex::new(State::new(workspaces)));
    match server {
        Ok((command, launch)) => acp::supervise(command, launch, state.clone()).await,
        Err(error) => {
            let reason = format!("{error:#}");
            eprintln!("ur daemon: cannot start the server: {reason}");
            state.lock().unwrap().set_server(Err(reason));
        }
    }
    server::serve(listener, Arc::default(), state, state_file).await
}
