use ur_client::Status;

/// `ur ls`: prints each workspace, then its sessions with their statuses.
pub async fn start() -> anyhow::Result<()> {
    let client = super::connect().await?;
    let (workspaces, sessions, _) = super::watch(&client).await?;
    for workspace in &workspaces {
        println!("{}  {}", workspace.name, workspace.path.display());
        for summary in sessions.iter().filter(|s| s.workspace == workspace.name) {
            let status = match summary.status {
                Status::Idle { .. } => "idle",
                Status::Working => "working",
                Status::NeedsPermission { .. } => "needs permission",
                Status::Failed { .. } => "failed",
            };
            let unread = if summary.unread { "  unread" } else { "" };
            println!("  {}  {status}{unread}", summary.session);
        }
    }
    Ok(())
}
