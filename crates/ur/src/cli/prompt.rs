use agent_client_protocol::schema::v1::{ContentBlock, SessionId};
use anyhow::bail;
use ur_client::{Request, Response};

/// `ur prompt <session> <text>`: sends one text block and returns once it is
/// sent. `ur read --follow` shows the reply.
pub async fn start(session: String, text: String) -> anyhow::Result<()> {
    let client = super::connect().await?;
    let request = Request::Prompt {
        session: SessionId::from(session.clone()),
        content: vec![ContentBlock::from(text)],
    };
    match super::request(&client, request).await? {
        Response::Done => Ok(()),
        Response::Busy => bail!("session {session} is busy"),
        other => Err(super::unexpected(other)),
    }
}
