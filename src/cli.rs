//! Clap-based argument parsing and output formatting.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::manifest::{ManifestEntry, WorkspaceManifest};

#[derive(Parser, Debug)]
#[command(name = "ur")]
pub struct Cli {
    /// Workspace directory.
    #[arg(short, long)]
    pub workspace: Option<PathBuf>,

    /// Enable verbose logging output.
    #[arg(short, long, global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Manage extensions.
    Extension {
        #[command(subcommand)]
        action: ExtensionAction,
    },
    /// Manage model role mappings.
    Role {
        #[command(subcommand)]
        action: RoleAction,
    },
    /// Run a single agent turn.
    Run {
        /// The user message to send.
        message: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum ExtensionAction {
    /// List all discovered extensions.
    List,
    /// Enable an extension by id.
    Enable { id: String },
    /// Disable an extension by id.
    Disable { id: String },
    /// Show details for an extension.
    Inspect { id: String },
}

#[derive(Subcommand, Debug)]
pub enum RoleAction {
    /// Show all role mappings.
    List,
    /// Show what a role resolves to.
    Get {
        /// Role name (e.g. "default", "fast").
        role: String,
    },
    /// Map a role to a provider/model pair.
    Set {
        /// Role name (e.g. "default", "fast").
        role: String,
        /// Provider/model reference (e.g. "google/gemini-3-flash-preview").
        model_ref: String,
    },
}

/// Prints extensions in a tabular format.
pub fn print_list(manifest: &WorkspaceManifest) {
    println!("{:<20}{:<20}{:<11}ENABLED", "ID", "NAME", "SOURCE");
    for ext in &manifest.extensions {
        print_row(ext);
    }
}

/// Prints detailed information about a single extension.
pub fn print_inspect(entry: &ManifestEntry) {
    println!("id:           {}", entry.id);
    println!("name:         {}", entry.name);
    println!("source:       {}", entry.source);
    println!("path:         {}", entry.dir_path);
    println!("enabled:      {}", entry.enabled);
    println!(
        "capabilities: {}",
        if entry.capabilities.is_empty() {
            "(none)".to_owned()
        } else {
            entry.capabilities.join(", ")
        }
    );
}

fn print_row(ext: &ManifestEntry) {
    let enabled_display = if ext.enabled { "\u{2713}" } else { "\u{2717}" };
    println!(
        "{:<20}{:<20}{:<11}{}",
        ext.id, ext.name, ext.source, enabled_display
    );
}
