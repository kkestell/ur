//! Requests that call the server or write the state file. Each handles the
//! server's response in an `on_receiving_result` callback, which the SDK runs
//! before it dispatches the server's next message. The callbacks always return
//! `Ok`, because an error from one shuts down the ACP connection.

use std::path::Path;
use std::sync::{Arc, Mutex};

use agent_client_protocol::schema::v1::{
    CancelNotification, ContentBlock, NewSessionRequest, PromptRequest, SessionId,
};
use agent_client_protocol::{Agent, ConnectionTo};
use anyhow::bail;
use ur_client::{DaemonMessage, Frame, Response, Workspace};

use super::acp;
use super::server::Outbox;
use super::state::State;
use super::state_file;

/// Adds a workspace whose path is an absolute path to a directory, and saves
/// it in the state file.
pub fn add_workspace(
    state: &Arc<Mutex<State>>,
    state_file: &Path,
    workspace: Workspace,
) -> anyhow::Result<()> {
    let path = &workspace.path;
    if !path.is_absolute() {
        bail!("workspace path {} is not absolute", path.display());
    }
    if !path.is_dir() {
        bail!("workspace path {} is not a directory", path.display());
    }
    state
        .lock()
        .unwrap()
        .add_workspace(workspace, |workspaces| {
            state_file::write(state_file, workspaces)
        })
}

/// Removes a workspace from the state file, then cancels its running prompts
/// and removes it and its sessions.
pub fn remove_workspace(
    state: &Arc<Mutex<State>>,
    state_file: &Path,
    name: &str,
) -> anyhow::Result<()> {
    state.lock().unwrap().remove_workspace(
        name,
        |workspaces| state_file::write(state_file, workspaces),
        send_cancel,
    )
}

/// Sends `session/new` in the workspace and answers request `id` when the
/// server responds. The session is in `State` before the server's next update
/// is handled, so an update sent right after the response is kept.
pub fn new_session(
    state: &Arc<Mutex<State>>,
    workspace: String,
    id: u64,
    outbox: Outbox,
) -> anyhow::Result<()> {
    let (connection, path) = {
        let state = state.lock().unwrap();
        (state.connection()?, state.workspace_path(&workspace)?)
    };
    let state = state.clone();
    connection
        .send_request(NewSessionRequest::new(path))
        .on_receiving_result(move |result| async move {
            let response = match result {
                Ok(response) => {
                    let session = response.session_id;
                    match state
                        .lock()
                        .unwrap()
                        .add_session(session.clone(), workspace)
                    {
                        Ok(()) => Response::SessionCreated { session },
                        Err(error) => Response::Error {
                            message: format!("{error:#}"),
                        },
                    }
                }
                Err(error) => Response::Error {
                    message: format!("session/new failed: {}", acp::describe(&error)),
                },
            };
            outbox.send(Frame::json(&DaemonMessage::Response { id, response }));
            Ok(())
        })?;
    Ok(())
}

/// Sends `session/prompt` and answers once it is sent, or answers busy. The
/// operation guard is released after the turn's last update is in the
/// transcript.
pub fn prompt(
    state: &Arc<Mutex<State>>,
    session: SessionId,
    content: Vec<ContentBlock>,
) -> anyhow::Result<Response> {
    let callback_state = state.clone();
    state
        .lock()
        .unwrap()
        .start_prompt(&session, content, |connection, content| {
            let session = session.clone();
            connection
                .send_request(PromptRequest::new(session.clone(), content))
                .on_receiving_result(move |result| async move {
                    let result = result
                        .map(|response| response.stop_reason)
                        .map_err(|error| acp::describe(&error));
                    callback_state
                        .lock()
                        .unwrap()
                        .finish_prompt(&session, result);
                    Ok(())
                })
        })
}

pub fn cancel(state: &Arc<Mutex<State>>, session: &SessionId) -> anyhow::Result<()> {
    state.lock().unwrap().cancel(session, send_cancel)
}

fn send_cancel(
    connection: &ConnectionTo<Agent>,
    session: &SessionId,
) -> agent_client_protocol::Result<()> {
    connection.send_notification(CancelNotification::new(session.clone()))
}
