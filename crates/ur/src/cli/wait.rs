use agent_client_protocol::schema::v1::SessionId;
use anyhow::{anyhow, bail};
use ur_client::{Event, SessionSummary, Status};

#[derive(Clone, Copy, clap::ValueEnum)]
pub enum Until {
    /// No turn is running: the session is idle or failed.
    Idle,
    /// The session needs permission, failed, or is unread.
    Attention,
}

/// `ur wait <session> [--until attention|idle]`: waits until the condition
/// holds, then prints the session status as one JSON line.
pub async fn start(session: String, until: Until) -> anyhow::Result<()> {
    let session = SessionId::from(session);
    let client = super::connect().await?;
    let (_, sessions, mut events) = super::watch(&client).await?;
    let mut summary = sessions
        .into_iter()
        .find(|summary| summary.session == session)
        .ok_or_else(|| anyhow!("no session {session}"))?;
    while !holds(&summary, until) {
        match events.recv().await {
            Some(Event::SessionChanged { summary: changed }) if changed.session == session => {
                summary = changed;
            }
            Some(Event::WorkspaceRemoved { name }) if name == summary.workspace => {
                bail!("session {session} was removed with workspace {name}");
            }
            Some(_) => {}
            None => bail!("the daemon closed the connection"),
        }
    }
    println!("{}", serde_json::to_string(&summary.status)?);
    Ok(())
}

fn holds(summary: &SessionSummary, until: Until) -> bool {
    match until {
        Until::Idle => matches!(summary.status, Status::Idle { .. } | Status::Failed { .. }),
        Until::Attention => {
            summary.unread
                || matches!(
                    summary.status,
                    Status::NeedsPermission { .. } | Status::Failed { .. }
                )
        }
    }
}
