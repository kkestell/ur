use std::path::PathBuf;

use clap::{Parser, Subcommand};
use cli::wait::Until;

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
    /// Add or remove a workspace.
    #[command(subcommand)]
    Workspace(WorkspaceCommand),
    /// List workspaces and their sessions with their statuses.
    Ls,
    /// Create a session in a workspace and print its ID.
    New { workspace: String },
    /// Send a text prompt to a session.
    Prompt { session: String, text: String },
    /// Print a session's transcript as JSON lines.
    Read {
        session: String,
        /// Keep printing new entries until the session is removed or the
        /// daemon closes the connection.
        #[arg(long)]
        follow: bool,
    },
    /// Cancel a session's running prompt.
    Cancel { session: String },
    /// Answer a session's oldest pending permission request with its first
    /// allow option.
    Approve { session: String },
    /// Answer a session's oldest pending permission request with its first
    /// reject option.
    Deny { session: String },
    /// Wait until a condition holds for a session, then print its status as
    /// JSON.
    Wait {
        session: String,
        #[arg(long, value_enum, default_value_t = Until::Idle)]
        until: Until,
    },
}

#[derive(Subcommand)]
enum WorkspaceCommand {
    /// Add a workspace with a name and a directory.
    Add { name: String, path: PathBuf },
    /// Cancel a workspace's running prompts, stop its terminals, and remove it
    /// with its sessions.
    Rm { name: String },
}

fn main() -> anyhow::Result<()> {
    let command = Cli::parse().command;
    let runtime = tokio::runtime::Runtime::new()?;
    let result = runtime.block_on(async {
        match command {
            Command::Daemon => daemon::start(&ur_client::socket_path()).await,
            Command::AgentRun { workspace, prompt } => one_shot::start(&workspace, prompt).await,
            Command::Workspace(WorkspaceCommand::Add { name, path }) => {
                cli::workspace::add(name, &path).await
            }
            Command::Workspace(WorkspaceCommand::Rm { name }) => cli::workspace::rm(name).await,
            Command::Ls => cli::ls::start().await,
            Command::New { workspace } => cli::new::start(workspace).await,
            Command::Prompt { session, text } => cli::prompt::start(session, text).await,
            Command::Read { session, follow } => cli::read::start(session, follow).await,
            Command::Cancel { session } => cli::cancel::start(session).await,
            Command::Approve { session } => cli::answer::approve(session).await,
            Command::Deny { session } => cli::answer::deny(session).await,
            Command::Wait { session, until } => cli::wait::start(session, until).await,
        }
    });
    // Tokio reads stdin with a blocking read that cannot be cancelled. If
    // `agent-run` ends while waiting for an answer, such as when the server
    // exits, waiting for that read would hold the process until the next line.
    runtime.shutdown_background();
    result
}
