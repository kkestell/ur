use std::path::Path;
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
mod terminal;
#[cfg(test)]
mod tests;

/// Binds the socket, removing a stale one, then runs the daemon with the
/// server from the config file.
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
        AcpAgent::new(AcpAgentConfig::new(config.server.command).args(config.server.args))
    });
    run(listener, server).await
}

/// Connects to the server, then serves daemon clients. Daemon clients that
/// connect meanwhile wait in the listen backlog, so no request sees a server
/// that is still starting. Without a server, terminals and the transcripts
/// held in memory still work.
async fn run(
    listener: UnixListener,
    server: anyhow::Result<impl ConnectTo<Client> + 'static>,
) -> anyhow::Result<()> {
    let state = Arc::new(Mutex::new(State::default()));
    match server {
        Ok(server) => acp::connect(server, state.clone()).await,
        Err(error) => {
            let reason = format!("{error:#}");
            eprintln!("ur daemon: cannot start the server: {reason}");
            state.lock().unwrap().set_server(Err(reason));
        }
    }
    server::serve(listener, Arc::default(), state).await
}
