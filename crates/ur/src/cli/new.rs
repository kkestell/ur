use ur_client::{Request, Response};

/// `ur new <workspace>`: creates a session and prints its ID.
pub async fn start(workspace: String) -> anyhow::Result<()> {
    let client = super::connect().await?;
    match super::request(&client, Request::NewSession { workspace }).await? {
        Response::SessionCreated { session } => {
            println!("{session}");
            Ok(())
        }
        other => Err(super::unexpected(other)),
    }
}
