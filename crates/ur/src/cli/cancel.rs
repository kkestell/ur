use agent_client_protocol::schema::v1::SessionId;
use ur_client::{Request, Response};

/// `ur cancel <session>`: cancels the running prompt, if any.
pub async fn start(session: String) -> anyhow::Result<()> {
    let client = super::connect().await?;
    let request = Request::Cancel {
        session: SessionId::from(session),
    };
    match super::request(&client, request).await? {
        Response::Done => Ok(()),
        other => Err(super::unexpected(other)),
    }
}
