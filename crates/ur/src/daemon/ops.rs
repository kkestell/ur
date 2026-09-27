//! Requests that call the server or write the state file. Each handles the
//! server's response in an `on_receiving_result` callback, which the SDK runs
//! before it dispatches the server's next message. The callbacks always return
//! `Ok`, because an error from one shuts down the ACP connection. Prompt and
//! load callbacks carry the generation they were sent in, and ignore the error
//! a request gets when the ACP connection closes: the supervisor records that
//! server exit, so every interrupted turn gets one turn error entry.

use std::path::Path;
use std::sync::{Arc, Mutex};

use agent_client_protocol::schema::v1::{
    CancelNotification, ContentBlock, DeleteSessionRequest, LoadSessionRequest, NewSessionRequest,
    PromptRequest, SessionConfigId, SessionConfigOptionValue, SessionId,
    SetSessionConfigOptionRequest,
};
use agent_client_protocol::{Agent, ConnectionTo, is_incoming_transport_closed};
use anyhow::bail;
use ur_client::{Response, SessionKey, Workspace, WorkspaceColor};

use super::acp;
use super::server::Outbox;
use super::state::{State, respond};
use super::state_file;
use super::terminal::Terminals;

/// Adds a workspace whose path is an absolute path to a directory, and saves
/// it in the state file. For each connected server that can list sessions, a
/// spawned task adds the workspace's saved sessions.
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
    let connections = {
        let mut locked = state.lock().unwrap();
        locked.add_workspace(workspace.clone(), |workspaces| {
            state_file::write(state_file, workspaces)
        })?;
        locked.list_connections()
    };
    for (server, generation, connection) in connections {
        let state = state.clone();
        let workspace = workspace.clone();
        tokio::spawn(async move {
            match acp::list_sessions(&connection, workspace.path).await {
                Ok(sessions) => state.lock().unwrap().add_saved_sessions(
                    &server,
                    generation,
                    &workspace.name,
                    sessions,
                ),
                Err(error) => eprintln!(
                    "ur daemon: listing the sessions of {}: {error:#}",
                    workspace.name
                ),
            }
        });
    }
    Ok(())
}

/// Saves the workspace's new color in the state file.
pub fn set_workspace_color(
    state: &Arc<Mutex<State>>,
    state_file: &Path,
    name: &str,
    color: WorkspaceColor,
) -> anyhow::Result<()> {
    state
        .lock()
        .unwrap()
        .set_workspace_color(name, color, |workspaces| {
            state_file::write(state_file, workspaces)
        })
}

/// Removes a workspace from the state file, then cancels its running prompts,
/// removes it and its sessions, and closes its terminals.
pub fn remove_workspace(
    state: &Arc<Mutex<State>>,
    terminals: &Terminals,
    state_file: &Path,
    name: &str,
) -> anyhow::Result<()> {
    let removed = state.lock().unwrap().remove_workspace(
        name,
        |workspaces| state_file::write(state_file, workspaces),
        send_cancel,
    )?;
    for terminal in removed {
        // A shell that exited meanwhile is already gone.
        if let Err(error) = terminals.close(terminal) {
            eprintln!("ur daemon: {error:#}");
        }
    }
    Ok(())
}

/// Sends `session/new` in the workspace and answers request `id` when the
/// server responds. The session is in `State` before the server's next update
/// is handled, so an update sent right after the response is kept.
pub fn new_session(
    state: &Arc<Mutex<State>>,
    server: String,
    workspace: String,
    id: u64,
    outbox: Outbox,
) -> anyhow::Result<()> {
    let (generation, connection, path) = {
        let state = state.lock().unwrap();
        let (generation, connection) = state.server_connection(&server)?;
        (generation, connection, state.workspace_path(&workspace)?)
    };
    let state = state.clone();
    connection
        .send_request(NewSessionRequest::new(path))
        .on_receiving_result(move |result| async move {
            let response = match result {
                Ok(response) => {
                    let session = response.session_id;
                    match state.lock().unwrap().add_session(
                        &server,
                        generation,
                        session.clone(),
                        workspace,
                        response.config_options.unwrap_or_default(),
                    ) {
                        Ok(()) => Response::SessionCreated {
                            session: SessionKey::new(&server, &session),
                        },
                        Err(error) => Response::Error {
                            message: format!("{error:#}"),
                        },
                    }
                }
                Err(error) => Response::Error {
                    message: format!("session/new failed: {}", acp::describe(&error)),
                },
            };
            respond(&outbox, id, response);
            Ok(())
        })?;
    Ok(())
}

/// Subscribes to a session, and answers request `id` now or, when the
/// subscription starts or waits for a load, once the load ends. Returns the
/// response, or `None` when the load answers it later.
pub fn subscribe(
    state: &Arc<Mutex<State>>,
    session: &SessionKey,
    id: u64,
    outbox: Outbox,
) -> anyhow::Result<Option<Response>> {
    let owner = state.lock().unwrap().session_owner(session)?;
    state
        .lock()
        .unwrap()
        .subscribe(session, id, outbox, load(state, None, owner))
}

/// Sends `session/prompt` and answers once it is sent, or answers busy. A
/// session that is not loaded is loaded first. The operation guard is released
/// after the turn's last update is in the transcript.
pub fn prompt(
    state: &Arc<Mutex<State>>,
    session: SessionKey,
    content: Vec<ContentBlock>,
) -> anyhow::Result<Response> {
    let owner = state.lock().unwrap().session_owner(&session)?;
    let load = load(state, Some(content.clone()), owner.clone());
    state
        .lock()
        .unwrap()
        .prompt(&session, content, send_prompt(state, owner), load)
}

/// Builds the function that sends `session/load`. Its callback ends the load
/// and then, if `prompt` holds a prompt's content and the load succeeded,
/// sends the prompt under the same lock, so no other request can take the
/// operation guard in between.
pub fn load(
    state: &Arc<Mutex<State>>,
    prompt: Option<Vec<ContentBlock>>,
    owner: String,
) -> impl FnOnce(&ConnectionTo<Agent>, u64, LoadSessionRequest) -> agent_client_protocol::Result<()>
{
    let state = state.clone();
    move |connection, generation, request| {
        let session = SessionKey::new(&owner, &request.session_id);
        connection
            .send_request(request)
            .on_receiving_result(move |result| async move {
                if let Err(error) = &result
                    && is_incoming_transport_closed(error)
                {
                    return Ok(());
                }
                let result = result
                    .map(|response| response.config_options)
                    .map_err(|error| acp::describe(&error));
                let mut locked = state.lock().unwrap();
                if locked.finish_load(&owner, &session, generation, result)
                    && let Some(content) = prompt
                    && let Err(error) =
                        locked.start_prompt(&session, content, send_prompt(&state, owner.clone()))
                {
                    eprintln!("ur daemon: prompting {session} after its load: {error:#}");
                }
                Ok(())
            })
    }
}

/// Builds the function that sends `session/prompt`. Its callback ends the turn
/// and then sends the waiting delete's `session/delete`, if there is one,
/// under the same lock, so no other request can take the operation guard in
/// between.
fn send_prompt(
    state: &Arc<Mutex<State>>,
    owner: String,
) -> impl FnOnce(&ConnectionTo<Agent>, u64, PromptRequest) -> agent_client_protocol::Result<()> {
    let state = state.clone();
    move |connection, generation, request| {
        let session = SessionKey::new(&owner, &request.session_id);
        connection
            .send_request(request)
            .on_receiving_result(move |result| async move {
                if let Err(error) = &result
                    && is_incoming_transport_closed(error)
                {
                    return Ok(());
                }
                let result = result
                    .map(|response| response.stop_reason)
                    .map_err(|error| acp::describe(&error));
                let mut locked = state.lock().unwrap();
                if let Some((id, outbox)) =
                    locked.finish_prompt(&owner, &session, generation, result)
                    && let Err(error) = locked.start_delete(
                        &session,
                        id,
                        outbox.clone(),
                        send_delete(&state, owner.clone(), id, outbox.clone()),
                    )
                {
                    respond(
                        &outbox,
                        id,
                        Response::Error {
                            message: format!("{error:#}"),
                        },
                    );
                }
                Ok(())
            })
    }
}

/// Deletes a session, and answers request `id` when `session/delete` returns.
/// A running turn is cancelled first, and `session/delete` is sent when it
/// ends. Returns the response, or `None` when the delete answers it later.
pub fn delete_session(
    state: &Arc<Mutex<State>>,
    session: &SessionKey,
    id: u64,
    outbox: Outbox,
) -> anyhow::Result<Option<Response>> {
    let owner = state.lock().unwrap().session_owner(session)?;
    let send_delete = send_delete(state, owner, id, outbox.clone());
    state
        .lock()
        .unwrap()
        .delete_session(session, id, outbox, send_cancel, send_delete)
}

/// Builds the function that sends `session/delete`. Its callback ends the
/// delete and answers request `id`.
fn send_delete(
    state: &Arc<Mutex<State>>,
    owner: String,
    id: u64,
    outbox: Outbox,
) -> impl FnOnce(&ConnectionTo<Agent>, u64, DeleteSessionRequest) -> agent_client_protocol::Result<()>
{
    let state = state.clone();
    move |connection, generation, request| {
        let session = SessionKey::new(&owner, &request.session_id);
        connection
            .send_request(request)
            .on_receiving_result(move |result| async move {
                let result = result
                    .map(|_| ())
                    .map_err(|error| format!("session/delete failed: {}", acp::describe(&error)));
                let handled = state.lock().unwrap().finish_delete(
                    &owner,
                    &session,
                    generation,
                    result.clone(),
                );
                if handled {
                    let response = match result {
                        Ok(()) => Response::Done,
                        Err(message) => Response::Error { message },
                    };
                    respond(&outbox, id, response);
                }
                Ok(())
            })
    }
}

/// Sends `session/set_config_option` and answers request `id` when the server
/// responds, with the new config options applied.
pub fn set_config_option(
    state: &Arc<Mutex<State>>,
    session: SessionKey,
    config_id: SessionConfigId,
    value: SessionConfigOptionValue,
    id: u64,
    outbox: Outbox,
) -> anyhow::Result<()> {
    let (owner, generation, connection, acp_id) =
        state.lock().unwrap().session_connection(&session)?;
    let state = state.clone();
    connection
        .send_request(SetSessionConfigOptionRequest::new(acp_id, config_id, value))
        .on_receiving_result(move |result| async move {
            let response = match result {
                Ok(response) => match state.lock().unwrap().set_config_options(
                    &owner,
                    generation,
                    &session,
                    response.config_options,
                ) {
                    Ok(()) => Response::Done,
                    Err(error) => Response::Error {
                        message: format!("{error:#}"),
                    },
                },
                Err(error) => Response::Error {
                    message: format!(
                        "session/set_config_option failed: {}",
                        acp::describe(&error)
                    ),
                },
            };
            respond(&outbox, id, response);
            Ok(())
        })?;
    Ok(())
}

pub fn cancel(state: &Arc<Mutex<State>>, session: &SessionKey) -> anyhow::Result<()> {
    state.lock().unwrap().cancel(session, send_cancel)
}

fn send_cancel(
    connection: &ConnectionTo<Agent>,
    session: &SessionId,
) -> agent_client_protocol::Result<()> {
    connection.send_notification(CancelNotification::new(session.clone()))
}
