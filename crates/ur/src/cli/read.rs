use agent_client_protocol::schema::v1::SessionId;
use anyhow::bail;
use ur_client::{Entry, Event, Request, Response};

/// `ur read <session> [--follow]`: prints each transcript entry as one JSON
/// line. With `follow`, keeps printing later entries until the daemon closes
/// the socket connection.
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
    if !follow {
        return Ok(());
    }
    while let Some(event) = events.recv().await {
        if let Event::Entry { entry, .. } = event {
            print(&entry)?;
        }
    }
    Ok(())
}

fn print(entry: &Entry) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string(entry)?);
    Ok(())
}
