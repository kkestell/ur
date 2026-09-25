use agent_client_protocol::{ConnectTo, Stdio};
use ur_fake_server::{Hold, SavedHistory, fake_server};

/// The fake server over stdin and stdout, for the daemon to launch from the
/// config file. Nothing releases its `hold` script. Its saved history lasts as
/// long as the process, or, given a file as its argument, is kept in that file.
#[tokio::main]
async fn main() -> agent_client_protocol::Result<()> {
    let history = match std::env::args_os().nth(1) {
        Some(file) => SavedHistory::file(file.into()),
        None => SavedHistory::default(),
    };
    fake_server(Hold::default(), history)
        .connect_to(Stdio::new())
        .await
}
