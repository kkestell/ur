use agent_client_protocol::{ConnectTo, Stdio};
use ur_fake_server::{Hold, SavedHistory, fake_server};

/// The fake server over stdin and stdout, for the daemon to launch from the
/// config file. Nothing releases its `hold` script, and its saved history
/// lasts as long as the process.
#[tokio::main]
async fn main() -> agent_client_protocol::Result<()> {
    fake_server(Hold::default(), SavedHistory::default())
        .connect_to(Stdio::new())
        .await
}
