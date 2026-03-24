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
    /// Manage extension configuration.
    Config {
        /// Extension ID (e.g. "llm-google").
        id: String,
        #[command(subcommand)]
        action: ExtConfigAction,
    },
}

#[derive(Subcommand, Debug)]
pub enum ExtConfigAction {
    /// List settings (optionally filtered by glob pattern).
    List {
        /// Glob pattern to filter keys (e.g. "gemini-flash.*").
        pattern: Option<String>,
    },
    /// Get a setting value.
    Get {
        /// Setting key (e.g. "gemini-flash.thinking_level").
        key: String,
    },
    /// Set a setting value.
    Set {
        /// Setting key (e.g. "gemini-flash.thinking_level").
        key: String,
        /// Setting value. Omit to be prompted for secrets.
        value: Option<String>,
    },
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
