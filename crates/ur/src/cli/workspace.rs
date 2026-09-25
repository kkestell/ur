use std::path::Path;

use ur_client::{Request, Response};

/// `ur workspace add <name> <path>`: adds a workspace, with `path` made
/// absolute.
pub async fn add(name: String, path: &Path) -> anyhow::Result<()> {
    let client = super::connect().await?;
    let path = std::path::absolute(path)?;
    done(super::request(&client, Request::AddWorkspace { name, path }).await?)
}

/// `ur workspace rm <name>`: cancels the workspace's running prompts and
/// removes it with its sessions.
pub async fn rm(name: String) -> anyhow::Result<()> {
    let client = super::connect().await?;
    done(super::request(&client, Request::RemoveWorkspace { name }).await?)
}

fn done(response: Response) -> anyhow::Result<()> {
    match response {
        Response::Done => Ok(()),
        other => Err(super::unexpected(other)),
    }
}
