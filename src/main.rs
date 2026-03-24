mod cli;
mod config;
mod discovery;
mod extension_host;
mod extension_settings;
mod keyring;
mod manifest;
mod model;
mod provider;
mod slot;
mod turn;

use std::env;
use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use mimalloc::MiMalloc;
use tracing_subscriber::EnvFilter;
use wasmtime::Engine;

use cli::{Cli, Command, ExtConfigAction, ExtensionAction, RoleAction};

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

fn main() -> Result<()> {
    let args = Cli::parse();

    if args.verbose {
        tracing_subscriber::fmt()
            .with_env_filter(
                EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("ur=debug")),
            )
            .with_target(true)
            .with_writer(std::io::stderr)
            .init();
    }

    let ur_root = env::var("UR_ROOT").map_or_else(|_| dirs_home().join(".ur"), PathBuf::from);

    let workspace = args
        .workspace
        .unwrap_or_else(|| env::current_dir().expect("cannot determine current directory"));

    let workspace = std::fs::canonicalize(&workspace)?;

    let engine = {
        let cache = wasmtime::Cache::new(wasmtime::CacheConfig::new())?;
        let mut config = wasmtime::Config::new();
        config.cache(Some(cache));
        Engine::new(&config)?
    };

    match args.command {
        Command::Extension { action } => match action {
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
            ExtensionAction::Config { id, action } => {
                let m = manifest::scan_and_load(&engine, &ur_root, &workspace)?;
                match action {
                    ExtConfigAction::List { pattern } => {
                        extension_settings::cmd_config_list(
                            &engine,
                            &ur_root,
                            &m,
                            &id,
                            pattern.as_deref(),
                        )?;
                    }
                    ExtConfigAction::Get { key } => {
                        extension_settings::cmd_config_get(&engine, &ur_root, &m, &id, &key)?;
                    }
                    ExtConfigAction::Set { key, value } => {
                        extension_settings::cmd_config_set(
                            &engine,
                            &ur_root,
                            &m,
                            &id,
                            &key,
                            value.as_deref(),
                        )?;
                    }
                }
            }
        },
        Command::Role { action } => {
            let m = manifest::scan_and_load(&engine, &ur_root, &workspace)?;
            let providers = model::collect_provider_models(&engine, &m)?;
            let mut config = config::UserConfig::load(&ur_root)?;

            match action {
                RoleAction::List => model::cmd_list(&config, &providers)?,
                RoleAction::Get { role } => model::cmd_get(&config, &providers, &role)?,
                RoleAction::Set { role, model_ref } => {
                    model::cmd_set(&ur_root, &mut config, &providers, &role, &model_ref)?;
                }
            }
        }
        Command::Run => {
            turn::run(&engine, &ur_root, &workspace)?;
        }
    }

    Ok(())
}

/// Returns the user's home directory.
fn dirs_home() -> PathBuf {
    env::var("HOME").map(PathBuf::from).expect("HOME not set")
}
