use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, bail};
use tokio::net::{UnixListener, UnixStream};

mod server;
mod terminal;

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
    server::serve(listener, Arc::default()).await
}
