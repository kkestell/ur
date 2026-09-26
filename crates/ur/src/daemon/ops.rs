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
use ur_client::{Response, Workspace};

use super::acp;
use super::server::Outbox;
use super::state::{State, respond};
use super::state_file;
use super::terminal::Terminals;

/// Adds a workspace whose path is an absolute path to a directory, and saves
/// it in the state file. When the server can list sessions, a spawned task
/// adds the workspace's saved sessions.
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
    let connection = {
        let mut locked = state.lock().unwrap();
        locked.add_workspace(workspace.clone(), |workspaces| {
            state_file::write(state_file, workspaces)
        })?;
        locked
            .server()
            .ok()
            .filter(|server| server.can_list())
            .map(|server| server.connection.clone())
    };
    if let Some(connection) = connection {
        let state = state.clone();
        tokio::spawn(async move {
            match acp::list_sessions(&connection, workspace.path).await {
                Ok(sessions) => state
                    .lock()
                    .unwrap()
                    .add_saved_sessions(&workspace.name, sessions),
                Err(error) => eprintln!(
                    "ur daemon: listing the sessions of {}: {error:#}",
                    workspace.name
                ),
            }
        });
    }
    Ok(())
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
                    match state.lock().unwrap().add_session(
                        session.clone(),
                        workspace,
                        response.config_options.unwrap_or_default(),
                    ) {
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
    session: &SessionId,
    id: u64,
    outbox: Outbox,
) -> anyhow::Result<Option<Response>> {
    state
        .lock()
        .unwrap()
        .subscribe(session, id, outbox, load(state, None))
}

/// Sends `session/prompt` and answers once it is sent, or answers busy. A
/// session that is not loaded is loaded first. The operation guard is released
/// after the turn's last update is in the transcript.
pub fn prompt(
    state: &Arc<Mutex<State>>,
    session: SessionId,
    content: Vec<ContentBlock>,
) -> anyhow::Result<Response> {
    let load = load(state, Some(content.clone()));
    state
        .lock()
        .unwrap()
        .prompt(&session, content, send_prompt(state), load)
}

/// Builds the function that sends `session/load`. Its callback ends the load
/// and then, if `prompt` holds a prompt's content and the load succeeded,
/// sends the prompt under the same lock, so no other request can take the
/// operation guard in between.
pub fn load(
    state: &Arc<Mutex<State>>,
    prompt: Option<Vec<ContentBlock>>,
) -> impl FnOnce(&ConnectionTo<Agent>, u64, LoadSessionRequest) -> agent_client_protocol::Result<()>
{
    let state = state.clone();
    move |connection, generation, request| {
        let session = request.session_id.clone();
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
                if locked.finish_load(&session, generation, result)
                    && let Some(content) = prompt
                    && let Err(error) = locked.start_prompt(&session, content, send_prompt(&state))
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
) -> impl FnOnce(&ConnectionTo<Agent>, u64, PromptRequest) -> agent_client_protocol::Result<()> {
    let state = state.clone();
    move |connection, generation, request| {
        let session = request.session_id.clone();
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
                if let Some((id, outbox)) = locked.finish_prompt(&session, generation, result)
                    && let Err(error) =
                        locked.start_delete(&session, send_delete(&state, id, outbox.clone()))
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
    session: &SessionId,
    id: u64,
    outbox: Outbox,
) -> anyhow::Result<Option<Response>> {
    let send_delete = send_delete(state, id, outbox.clone());
    state
        .lock()
        .unwrap()
        .delete_session(session, id, outbox, send_cancel, send_delete)
}

/// Builds the function that sends `session/delete`. Its callback ends the
/// delete and answers request `id`.
fn send_delete(
    state: &Arc<Mutex<State>>,
    id: u64,
    outbox: Outbox,
) -> impl FnOnce(&ConnectionTo<Agent>, u64, DeleteSessionRequest) -> agent_client_protocol::Result<()>
{
    let state = state.clone();
    move |connection, generation, request| {
        let session = request.session_id.clone();
        connection
            .send_request(request)
            .on_receiving_result(move |result| async move {
                let result = result
                    .map(|_| ())
                    .map_err(|error| format!("session/delete failed: {}", acp::describe(&error)));
                state
                    .lock()
                    .unwrap()
                    .finish_delete(&session, generation, result.clone());
                let response = match result {
                    Ok(()) => Response::Done,
                    Err(message) => Response::Error { message },
                };
                respond(&outbox, id, response);
                Ok(())
            })
    }
}

/// Sends `session/set_config_option` and answers request `id` when the server
/// responds, with the new config options applied.
pub fn set_config_option(
    state: &Arc<Mutex<State>>,
    session: SessionId,
    config_id: SessionConfigId,
    value: SessionConfigOptionValue,
    id: u64,
    outbox: Outbox,
) -> anyhow::Result<()> {
    let connection = state.lock().unwrap().connection()?;
    let state = state.clone();
    connection
        .send_request(SetSessionConfigOptionRequest::new(
            session.clone(),
            config_id,
            value,
        ))
        .on_receiving_result(move |result| async move {
            let response = match result {
                Ok(response) => match state
                    .lock()
                    .unwrap()
                    .set_config_options(&session, response.config_options)
                {
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

pub fn cancel(state: &Arc<Mutex<State>>, session: &SessionId) -> anyhow::Result<()> {
    state.lock().unwrap().cancel(session, send_cancel)
}

fn send_cancel(
    connection: &ConnectionTo<Agent>,
    session: &SessionId,
) -> agent_client_protocol::Result<()> {
    connection.send_notification(CancelNotification::new(session.clone()))
}
