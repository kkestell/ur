use clap::{Parser, Subcommand};

mod daemon;

#[derive(Parser)]
#[command(name = "ur")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the daemon on `$UR_SOCKET`, else `$TMPDIR/ur.sock`.
    Daemon,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Daemon => daemon::start(&ur_client::socket_path()).await,
    }
}
