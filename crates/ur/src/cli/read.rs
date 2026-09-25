use agent_client_protocol::schema::v1::SessionId;
use anyhow::bail;
use ur_client::{Entry, Event, Request, Response};

/// `ur read <session> [--follow]`: prints each transcript entry as one JSON
/// line. With `follow`, keeps printing later entries until the session is
/// removed or the daemon closes the socket connection. It focuses the session
/// it shows, so reading clears the unread flag, and a followed session does
/// not become unread.
pub async fn start(session: String, follow: bool) -> anyhow::Result<()> {
    let session = SessionId::from(session);
    let client = super::connect().await?;
    let mut events = client.events();
    let request = Request::Subscribe {
        session: session.clone(),
    };
    match super::request(&client, request).await? {
        Response::Done => {}
        other => return Err(super::unexpected(other)),
    }
    // The daemon queues the session snapshot before the response.
    match events.recv().await {
        Some(Event::SessionSnapshot { transcript, .. }) => {
            for entry in &transcript {
                print(entry)?;
            }
        }
        other => bail!("expected the session snapshot, got {other:?}"),
    }
    let request = Request::Focus {
        sessions: vec![session],
    };
    match super::request(&client, request).await? {
        Response::Done => {}
        other => return Err(super::unexpected(other)),
    }
    if !follow {
        return Ok(());
    }
    while let Some(event) = events.recv().await {
        match event {
            Event::Entry { entry, .. } => print(&entry)?,
            Event::SessionRemoved { .. } => break,
            _ => {}
        }
    }
    Ok(())
}

fn print(entry: &Entry) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string(entry)?);
    Ok(())
}
