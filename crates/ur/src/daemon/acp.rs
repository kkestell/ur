use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    AgentCapabilities, Implementation, InitializeRequest, InitializeResponse, ListSessionsRequest,
    RequestPermissionRequest, SessionInfo, SessionNotification,
};
use agent_client_protocol::{
    Agent, Client, ConnectTo, ConnectionTo, on_receive_notification, on_receive_request,
};
use anyhow::{anyhow, bail};
use serde_json::Value;
use tokio::sync::Notify;
use tokio::time::{sleep, timeout};

use super::ops;
use super::state::{Server, State};

/// Daemon clients wait in the listen backlog until `initialize` finishes, so
/// without a limit a server that never answers would block terminals too.
/// It covers listing the saved sessions too.
const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(30);

/// The supervisor's first wait before starting the server again. Each failed
/// start doubles it, up to `MAX_RESTART_DELAY`, and a successful `initialize`
/// resets it.
const RESTART_DELAY: Duration = Duration::from_secs(1);
const MAX_RESTART_DELAY: Duration = Duration::from_secs(30);

/// Starts the supervisor, which starts the server with `launch`, and returns
/// once the first `initialize` has finished, failed, or gone unanswered for
/// `INITIALIZE_TIMEOUT`. Each time the ACP connection ends, `State` records
/// the reason, which names `command`, and the supervisor starts the server
/// again.
pub async fn supervise<S: ConnectTo<Client> + 'static>(
    command: String,
    launch: impl Fn() -> S + Send + 'static,
    state: Arc<Mutex<State>>,
) {
    let ready = Arc::new(Notify::new());
    tokio::spawn({
        let ready = ready.clone();
        async move {
            let mut delay = RESTART_DELAY;
            loop {
                let (initialized, reason) = connect(&command, launch(), &state, &ready).await;
                if initialized {
                    delay = RESTART_DELAY;
                }
                eprintln!("ur daemon: {reason}");
                state.lock().unwrap().server_exited(reason);
                ready.notify_one();
                sleep(delay).await;
                delay = (delay * 2).min(MAX_RESTART_DELAY);
            }
        }
    });
    ready.notified().await;
}

/// Runs one ACP connection: sends `initialize`, lists every workspace's saved
/// sessions, stores the ACP connection in `State`, reloads the subscribed
/// sessions, signals `ready`, and waits for the ACP connection to close.
/// Returns whether `initialize` succeeded, and why the ACP connection ended.
async fn connect(
    command: &str,
    server: impl ConnectTo<Client> + 'static,
    state: &Arc<Mutex<State>>,
    ready: &Notify,
) -> (bool, String) {
    let mut initialized = false;
    let result = Client
        .builder()
        .on_receive_notification(
            {
                let state = state.clone();
                // Applying the update here, without awaiting, holds the SDK's
                // dispatch loop only for the lock, and keeps updates in the
                // order the server sent them.
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
            let started = timeout(INITIALIZE_TIMEOUT, start(&connection, state))
                .await
                .unwrap_or_else(|_| {
                    Err(anyhow!(
                        "no answer after {} seconds",
                        INITIALIZE_TIMEOUT.as_secs()
                    ))
                });
            let (capabilities, saved) = match started {
                Ok(started) => started,
                Err(error) => return Ok(Err(error)),
            };
            {
                let mut locked = state.lock().unwrap();
                locked.set_server(Ok(Server {
                    connection: connection.clone(),
                    capabilities,
                }));
                for (workspace, sessions) in saved {
                    locked.add_saved_sessions(&workspace, sessions);
                }
                locked.reload_subscribed(|connection, generation, request| {
                    ops::load(state, None)(connection, generation, request)
                });
            }
            initialized = true;
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
    (initialized, reason)
}

/// Sends `initialize`, then lists the saved sessions of every workspace when
/// the server can. A workspace whose list fails shows only the sessions
/// already in `State`.
async fn start(
    connection: &ConnectionTo<Agent>,
    state: &Arc<Mutex<State>>,
) -> anyhow::Result<(AgentCapabilities, Vec<(String, Vec<SessionInfo>)>)> {
    let capabilities = initialize(connection).await?.agent_capabilities;
    let mut saved = Vec::new();
    if capabilities.session_capabilities.list.is_some() {
        let workspaces = state.lock().unwrap().workspaces();
        for workspace in workspaces {
            match list_sessions(connection, workspace.path).await {
                Ok(sessions) => saved.push((workspace.name, sessions)),
                Err(error) => eprintln!(
                    "ur daemon: listing the sessions of {}: {error:#}",
                    workspace.name
                ),
            }
        }
    }
    Ok((capabilities, saved))
}

/// Every page of `session/list` for one workspace path.
pub async fn list_sessions(
    connection: &ConnectionTo<Agent>,
    path: PathBuf,
) -> anyhow::Result<Vec<SessionInfo>> {
    let mut sessions = Vec::new();
    let mut cursor = None;
    loop {
        let request = ListSessionsRequest::new().cwd(path.clone()).cursor(cursor);
        let response = connection
            .send_request(request)
            .block_task()
            .await
            .map_err(|error| anyhow::Error::msg(describe(&error)))?;
        sessions.extend(response.sessions);
        match response.next_cursor {
            Some(next) => cursor = Some(next),
            None => return Ok(sessions),
        }
    }
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
