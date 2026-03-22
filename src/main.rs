mod cli;
mod discovery;
mod extension_host;
mod manifest;
mod slot;

use std::env;
use std::path::PathBuf;

use std::path::Path;

use anyhow::Result;
use clap::Parser;
use mimalloc::MiMalloc;
use wasmtime::Engine;

use cli::{Cli, Command, ExtensionAction};

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

fn main() -> Result<()> {
    let args = Cli::parse();

    let ur_root = env::var("UR_ROOT").map_or_else(|_| dirs_home().join(".ur"), PathBuf::from);

    let workspace = args
        .workspace
        .unwrap_or_else(|| env::current_dir().expect("cannot determine current directory"));

    let workspace = std::fs::canonicalize(&workspace)?;

    match args.command {
        Command::Extensions { action } => match action {
            ExtensionAction::List => {
                let m = manifest::scan_and_load(&ur_root, &workspace)?;
                cli::print_list(&m);
            }
            ExtensionAction::Enable { id } => {
                let mut m = manifest::scan_and_load(&ur_root, &workspace)?;
                manifest::enable(&mut m, &id)?;
                manifest::save_manifest(&ur_root, &workspace, &m)?;
                println!("Enabled {id}");
            }
            ExtensionAction::Disable { id } => {
                let mut m = manifest::scan_and_load(&ur_root, &workspace)?;
                manifest::disable(&mut m, &id)?;
                manifest::save_manifest(&ur_root, &workspace, &m)?;
                println!("Disabled {id}");
            }
            ExtensionAction::Inspect { id } => {
                let m = manifest::scan_and_load(&ur_root, &workspace)?;
                let entry = manifest::find_entry(&m, &id)?;
                cli::print_inspect(entry);
            }
            ExtensionAction::Check => {
                let m = manifest::scan_and_load(&ur_root, &workspace)?;
                let engine = Engine::default();
                check_extensions(&engine, &m)?;
            }
        },
    }

    Ok(())
}

/// Instantiates all enabled extensions and calls `init()`.
fn check_extensions(engine: &Engine, manifest: &manifest::WorkspaceManifest) -> Result<()> {
    for entry in &manifest.extensions {
        if !entry.enabled {
            continue;
        }
        let path = Path::new(&entry.wasm_path);
        let mut instance =
            extension_host::ExtensionInstance::load(engine, path, entry.slot.as_deref())
                .map_err(|e| anyhow::anyhow!("loading {}: {e}", entry.id))?;

        match instance.init(&[]) {
            Ok(Ok(())) => println!("{}: ok", entry.id),
            Ok(Err(e)) => println!("{}: init error: {e}", entry.id),
            Err(e) => println!("{}: trap: {e}", entry.id),
        }
    }
    Ok(())
}

/// Returns the user's home directory.
fn dirs_home() -> PathBuf {
    env::var("HOME").map(PathBuf::from).expect("HOME not set")
}
