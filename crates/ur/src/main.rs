use std::path::PathBuf;

use clap::{Parser, Subcommand};

mod config;
mod daemon;
mod one_shot;

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
    /// Run one prompt against the configured server without a daemon.
    AgentRun { workspace: PathBuf, prompt: String },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Daemon => daemon::start(&ur_client::socket_path()).await,
        Command::AgentRun { workspace, prompt } => one_shot::start(&workspace, prompt).await,
    }
}
