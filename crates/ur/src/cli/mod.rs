use anyhow::{Context, bail};
use ur_client::{Client, Request, Response};

pub mod new;
pub mod prompt;
pub mod read;

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

fn unexpected(response: Response) -> anyhow::Error {
    anyhow::anyhow!("unexpected response from the daemon: {response:?}")
}
