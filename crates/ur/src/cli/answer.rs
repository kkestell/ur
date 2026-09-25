use agent_client_protocol::schema::v1::{PermissionOptionKind, SessionId};
use anyhow::{anyhow, bail};
use ur_client::{Request, Response, Status};

/// `ur approve <session>`: answers the oldest pending permission request with
/// its first `allow_*` option.
pub async fn approve(session: String) -> anyhow::Result<()> {
    answer(session, "allow", |kind| {
        matches!(
            kind,
            PermissionOptionKind::AllowOnce | PermissionOptionKind::AllowAlways
        )
    })
    .await
}

/// `ur deny <session>`: answers the oldest pending permission request with its
/// first `reject_*` option.
pub async fn deny(session: String) -> anyhow::Result<()> {
    answer(session, "reject", |kind| {
        matches!(
            kind,
            PermissionOptionKind::RejectOnce | PermissionOptionKind::RejectAlways
        )
    })
    .await
}

async fn answer(
    session: String,
    kind_name: &str,
    kind: impl Fn(PermissionOptionKind) -> bool,
) -> anyhow::Result<()> {
    let session = SessionId::from(session);
    let client = super::connect().await?;
    let (_, sessions, _) = super::watch(&client).await?;
    let summary = sessions
        .into_iter()
        .find(|summary| summary.session == session)
        .ok_or_else(|| anyhow!("no session {session}"))?;
    let Status::NeedsPermission { requests } = summary.status else {
        bail!("session {session} has no pending permission request");
    };
    let pending = &requests[0];
    let option = pending
        .request
        .options
        .iter()
        .find(|option| kind(option.kind))
        .ok_or_else(|| {
            anyhow!(
                "permission request {} has no {kind_name} option",
                pending.request_id
            )
        })?;
    let request = Request::AnswerPermission {
        session,
        request_id: pending.request_id,
        option_id: option.option_id.clone(),
    };
    match super::request(&client, request).await? {
        Response::Done => Ok(()),
        other => Err(super::unexpected(other)),
    }
}
