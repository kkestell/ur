mod cli;
mod config;
mod discovery;
mod extension_host;
mod manifest;
mod model;
mod slot;
mod turn;

use std::env;
use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use mimalloc::MiMalloc;
use wasmtime::Engine;

use cli::{Cli, Command, ExtensionAction, ModelAction};

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

fn main() -> Result<()> {
    let args = Cli::parse();

    let ur_root = env::var("UR_ROOT").map_or_else(|_| dirs_home().join(".ur"), PathBuf::from);

    let workspace = args
        .workspace
        .unwrap_or_else(|| env::current_dir().expect("cannot determine current directory"));

    let workspace = std::fs::canonicalize(&workspace)?;

    let engine = Engine::default();

    match args.command {
        Command::Extensions { action } => match action {
            ExtensionAction::List => {
                let m = manifest::scan_and_load(&engine, &ur_root, &workspace)?;
                cli::print_list(&m);
            }
            ExtensionAction::Enable { id } => {
                let mut m = manifest::scan_and_load(&engine, &ur_root, &workspace)?;
                manifest::enable(&mut m, &id)?;
                manifest::save_manifest(&ur_root, &workspace, &m)?;
                println!("Enabled {id}");
            }
            ExtensionAction::Disable { id } => {
                let mut m = manifest::scan_and_load(&engine, &ur_root, &workspace)?;
                manifest::disable(&mut m, &id)?;
                manifest::save_manifest(&ur_root, &workspace, &m)?;
                println!("Disabled {id}");
            }
            ExtensionAction::Inspect { id } => {
                let m = manifest::scan_and_load(&engine, &ur_root, &workspace)?;
                let entry = manifest::find_entry(&m, &id)?;
                cli::print_inspect(entry);
            }
        },
        Command::Run => {
            turn::run(&engine, &ur_root, &workspace)?;
        }
        Command::Model { action } => {
            let m = manifest::scan_and_load(&engine, &ur_root, &workspace)?;
            let providers = model::collect_provider_models(&engine, &m)?;
            let mut config = config::UserConfig::load(&ur_root)?;

            match action {
                ModelAction::List => model::cmd_list(&config, &providers)?,
                ModelAction::Get { role } => model::cmd_get(&config, &providers, &role)?,
                ModelAction::Set { role, model_ref } => {
                    model::cmd_set(&ur_root, &mut config, &providers, &role, &model_ref)?;
                }
                ModelAction::Config { role } => {
                    model::cmd_config(&config, &providers, &role)?;
                }
                ModelAction::Setting { role, key, value } => {
                    model::cmd_setting(&ur_root, &mut config, &providers, &role, &key, &value)?;
                }
            }
        }
    }

    Ok(())
}

/// Returns the user's home directory.
fn dirs_home() -> PathBuf {
    env::var("HOME").map(PathBuf::from).expect("HOME not set")
}
