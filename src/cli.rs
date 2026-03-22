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

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Manage extensions.
    Extensions {
        #[command(subcommand)]
        action: ExtensionAction,
    },
    /// Manage model role mappings.
    Model {
        #[command(subcommand)]
        action: ModelAction,
    },
    /// Run a single agent turn (tracer bullet).
    Run,
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
pub enum ModelAction {
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
        /// Provider/model reference (e.g. "anthropic/claude-sonnet-4-6").
        model_ref: String,
    },
    /// Show available settings for a role's resolved model.
    Config {
        /// Role name to inspect.
        role: String,
    },
    /// Set a provider setting for a role's resolved model.
    Setting {
        /// Role name whose model to configure.
        role: String,
        /// Setting key (e.g. `thinking_budget`).
        key: String,
        /// Setting value (e.g. "8000", "high", "true").
        value: String,
    },
}

/// Prints extensions in a tabular format.
pub fn print_list(manifest: &WorkspaceManifest) {
    println!(
        "{:<17}{:<18}{:<21}{:<11}ENABLED",
        "ID", "NAME", "SLOT", "SOURCE"
    );
    for ext in &manifest.extensions {
        print_row(ext);
    }
}

/// Prints detailed information about a single extension.
pub fn print_inspect(entry: &ManifestEntry) {
    println!("id:       {}", entry.id);
    println!("name:     {}", entry.name);
    println!("slot:     {}", entry.slot.as_deref().unwrap_or("\u{2014}"));
    println!("source:   {}", entry.source);
    println!("path:     {}", entry.wasm_path);
    println!("checksum: {}", entry.checksum);
    println!("enabled:  {}", entry.enabled);
}

fn print_row(ext: &ManifestEntry) {
    let slot_display = ext.slot.as_deref().unwrap_or("\u{2014}");
    let enabled_display = if ext.enabled { "\u{2713}" } else { "\u{2717}" };
    println!(
        "{:<17}{:<18}{:<21}{:<11}{}",
        ext.id, ext.name, slot_display, ext.source, enabled_display
    );
}
