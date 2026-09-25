use std::path::Path;

use ur_client::{Request, Response};

/// `ur new <path>`: creates a session and prints its ID.
pub async fn start(path: &Path) -> anyhow::Result<()> {
    let client = super::connect().await?;
    let path = std::path::absolute(path)?;
    match super::request(&client, Request::NewSession { path }).await? {
        Response::SessionCreated { session } => {
            println!("{session}");
            Ok(())
        }
        other => Err(super::unexpected(other)),
    }
}
