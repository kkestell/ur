use ur_client::Status;

/// `ur ls`: prints each workspace, then its sessions with their statuses and
/// session titles.
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
            let title = match &summary.title {
                Some(title) => format!("  {title}"),
                None => String::new(),
            };
            let unread = if summary.unread { "  unread" } else { "" };
            println!("  {}  {status}{title}{unread}", summary.session);
        }
    }
    Ok(())
}
