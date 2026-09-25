//! Session requests that call the server. Each handles the server's response
//! in an `on_receiving_result` callback, which the SDK runs before it
//! dispatches the server's next message. The callbacks always return `Ok`,
//! because an error from one shuts down the ACP connection.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use agent_client_protocol::schema::v1::{
    ContentBlock, NewSessionRequest, PromptRequest, SessionId,
};
use ur_client::{DaemonMessage, Frame, Response};

use super::server::Outbox;
use super::state::State;

/// Sends `session/new` and answers request `id` when the server responds.
/// The session is in `State` before the server's next update is handled, so
/// an update sent right after the response is kept.
pub fn new_session(
    state: &Arc<Mutex<State>>,
    path: PathBuf,
    id: u64,
    outbox: Outbox,
) -> anyhow::Result<()> {
    let connection = state.lock().unwrap().connection()?;
    let state = state.clone();
    connection
        .send_request(NewSessionRequest::new(path))
        .on_receiving_result(move |result| async move {
            let response = match result {
                Ok(response) => {
                    let session = response.session_id;
                    state.lock().unwrap().add_session(session.clone());
                    Response::SessionCreated { session }
                }
                Err(error) => Response::Error {
                    message: format!("session/new failed: {error}"),
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
                    if let Err(error) = result {
                        eprintln!("ur daemon: session/prompt for {session} failed: {error}");
                    }
                    callback_state.lock().unwrap().finish_prompt(&session);
                    Ok(())
                })
        })
}
