use agent_client_protocol::{ConnectTo, Stdio};
use ur_fake_server::{Hold, fake_server};

/// The fake server over stdin and stdout, for the daemon to launch from the
/// config file. Nothing releases its `hold` script.
#[tokio::main]
async fn main() -> agent_client_protocol::Result<()> {
    fake_server(Hold::default()).connect_to(Stdio::new()).await
}
