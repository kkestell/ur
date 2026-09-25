use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    Implementation, InitializeRequest, InitializeResponse, RequestPermissionRequest,
    SessionNotification,
};
use agent_client_protocol::{
    Agent, Client, ConnectTo, ConnectionTo, on_receive_notification, on_receive_request,
};
use anyhow::{anyhow, bail};
use serde_json::Value;
use tokio::sync::Notify;
use tokio::time::timeout;

use super::state::State;

/// Daemon clients wait in the listen backlog until `initialize` finishes, so
/// without a limit a server that never answers would block terminals too.
const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(30);

/// Starts the ACP connection to the server and returns once `initialize` has
/// finished, failed, or gone unanswered for `INITIALIZE_TIMEOUT`. When the ACP
/// connection ends, `State` keeps the reason, which names `command`.
pub async fn connect(
    command: String,
    server: impl ConnectTo<Client> + 'static,
    state: Arc<Mutex<State>>,
) {
    let ready = Arc::new(Notify::new());
    tokio::spawn({
        let ready = ready.clone();
        async move {
            let result = Client
                .builder()
                .on_receive_notification(
                    {
                        let state = state.clone();
                        // Applying the update here, without awaiting, holds the
                        // SDK's dispatch loop only for the lock, and keeps
                        // updates in the order the server sent them.
                        async move |notification: SessionNotification, _connection| {
                            state.lock().unwrap().apply_update(notification);
                            Ok(())
                        }
                    },
                    on_receive_notification!(),
                )
                .on_receive_request(
                    {
                        let state = state.clone();
                        async move |request: RequestPermissionRequest, responder, _connection| {
                            state.lock().unwrap().request_permission(request, responder)
                        }
                    },
                    on_receive_request!(),
                )
                .connect_with(server, async |connection: ConnectionTo<Agent>| {
                    let initialized = timeout(INITIALIZE_TIMEOUT, initialize(&connection))
                        .await
                        .unwrap_or_else(|_| {
                            Err(anyhow!(
                                "no answer after {} seconds",
                                INITIALIZE_TIMEOUT.as_secs()
                            ))
                        });
                    if let Err(error) = initialized {
                        return Ok(Err(error));
                    }
                    state.lock().unwrap().set_server(Ok(connection.clone()));
                    ready.notify_one();
                    connection.incoming_closed().await;
                    Ok(Ok(()))
                })
                .await;
            let reason = match result {
                Ok(Ok(())) => format!("{command} closed the ACP connection"),
                Ok(Err(error)) => format!("initialize failed for {command}: {error:#}"),
                Err(error) => format!(
                    "the ACP connection to {command} failed: {}",
                    describe(&error)
                ),
            };
            eprintln!("ur daemon: {reason}");
            state.lock().unwrap().set_server(Err(reason));
            ready.notify_one();
        }
    });
    ready.notified().await;
}

/// Sends `initialize` and checks that the server speaks ACP version 1.
pub async fn initialize(connection: &ConnectionTo<Agent>) -> anyhow::Result<InitializeResponse> {
    let initialize = connection
        .send_request(
            InitializeRequest::new(ProtocolVersion::V1)
                .client_info(Implementation::new("ur", env!("CARGO_PKG_VERSION"))),
        )
        .block_task()
        .await
        .map_err(|error| anyhow::Error::msg(describe(&error)))?;
    if initialize.protocol_version != ProtocolVersion::V1 {
        bail!(
            "the server uses ACP version {}; ur supports only version 1",
            initialize.protocol_version
        );
    }
    Ok(initialize)
}

/// An SDK error's message and details on one line. The SDK wraps the details
/// of an error from one of its tasks with the task's source location, which
/// this leaves out.
pub fn describe(error: &agent_client_protocol::Error) -> String {
    let mut data = error.data.as_ref();
    while let Some(inner) = data.and_then(|data| data.get("spawned_at").and(data.get("data"))) {
        data = Some(inner);
    }
    match data {
        None | Some(Value::Null) => error.message.clone(),
        Some(Value::String(details)) => format!("{}: {details}", error.message),
        Some(details) => format!("{}: {details}", error.message),
    }
}
