use std::sync::{Arc, Mutex};

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    Implementation, InitializeRequest, InitializeResponse, RequestPermissionRequest,
    SessionNotification,
};
use agent_client_protocol::{
    Agent, Client, ConnectTo, ConnectionTo, on_receive_notification, on_receive_request,
};
use anyhow::bail;
use tokio::sync::Notify;

use super::state::State;

/// Starts the ACP connection to the server and returns once `initialize` has
/// finished or failed. When the ACP connection ends, `State` keeps the reason.
pub async fn connect(server: impl ConnectTo<Client> + 'static, state: Arc<Mutex<State>>) {
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
                    async move |_: RequestPermissionRequest, responder, _connection| {
                        responder.respond_with_internal_error(
                            "ur does not answer permission requests yet",
                        )
                    },
                    on_receive_request!(),
                )
                .connect_with(server, async |connection: ConnectionTo<Agent>| {
                    if let Err(error) = initialize(&connection).await {
                        return Ok(Err(error));
                    }
                    state.lock().unwrap().set_server(Ok(connection.clone()));
                    ready.notify_one();
                    connection.incoming_closed().await;
                    Ok(Ok(()))
                })
                .await;
            let reason = match result {
                Ok(Ok(())) => "the server closed the ACP connection".to_string(),
                Ok(Err(error)) => format!("initialize failed: {error:#}"),
                Err(error) => format!("the ACP connection failed: {error}"),
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
        .await?;
    if initialize.protocol_version != ProtocolVersion::V1 {
        bail!(
            "the server uses ACP version {}; ur supports only version 1",
            initialize.protocol_version
        );
    }
    Ok(initialize)
}
