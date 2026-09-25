use std::path::PathBuf;

use clap::{Parser, Subcommand};

mod cli;
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
    /// Create a session in `path` and print its ID.
    New { path: PathBuf },
    /// Send a text prompt to a session.
    Prompt { session: String, text: String },
    /// Print a session's transcript as JSON lines.
    Read {
        session: String,
        /// Keep printing new entries until the daemon closes the connection.
        #[arg(long)]
        follow: bool,
    },
}

fn main() -> anyhow::Result<()> {
    let command = Cli::parse().command;
    let runtime = tokio::runtime::Runtime::new()?;
    let result = runtime.block_on(async {
        match command {
            Command::Daemon => daemon::start(&ur_client::socket_path()).await,
            Command::AgentRun { workspace, prompt } => one_shot::start(&workspace, prompt).await,
            Command::New { path } => cli::new::start(&path).await,
            Command::Prompt { session, text } => cli::prompt::start(session, text).await,
            Command::Read { session, follow } => cli::read::start(session, follow).await,
        }
    });
    // Tokio reads stdin with a blocking read that cannot be cancelled. If
    // `agent-run` ends while waiting for an answer, such as when the server
    // exits, waiting for that read would hold the process until the next line.
    runtime.shutdown_background();
    result
}
