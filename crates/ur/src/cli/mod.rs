use anyhow::{Context, bail};
use tokio::sync::mpsc::UnboundedReceiver;
use ur_client::{Client, Event, Request, Response, SessionSummary, Workspace};

pub mod answer;
pub mod cancel;
pub mod ls;
pub mod new;
pub mod prompt;
pub mod read;
pub mod wait;
pub mod workspace;

/// Connects to the daemon on `$UR_SOCKET`, else `$TMPDIR/ur.sock`.
async fn connect() -> anyhow::Result<Client> {
    let path = ur_client::socket_path();
    Client::connect(&path)
        .await
        .with_context(|| format!("connecting to the daemon on {}", path.display()))
}

/// Sends a request, turning a daemon error into an error.
async fn request(client: &Client, request: Request) -> anyhow::Result<Response> {
    match client.request(request).await? {
        Response::Error { message } => bail!(message),
        response => Ok(response),
    }
}

/// Sends `watch` and returns the watch snapshot's workspaces and sessions, and
/// the receiver for the changes after it.
async fn watch(
    client: &Client,
) -> anyhow::Result<(
    Vec<Workspace>,
    Vec<SessionSummary>,
    UnboundedReceiver<Event>,
)> {
    let mut events = client.events();
    match request(client, Request::Watch).await? {
        Response::Done => {}
        other => return Err(unexpected(other)),
    }
    // The daemon queues the watch snapshot before the response.
    match events.recv().await {
        Some(Event::WatchSnapshot {
            workspaces,
            sessions,
            ..
        }) => Ok((workspaces, sessions, events)),
        other => bail!("expected the watch snapshot, got {other:?}"),
    }
}

fn unexpected(response: Response) -> anyhow::Error {
    anyhow::anyhow!("unexpected response from the daemon: {response:?}")
}
