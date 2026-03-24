mod app;
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
mod workspace;

use std::env;
use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use mimalloc::MiMalloc;
use tracing_subscriber::EnvFilter;

use app::UrApp;
use cli::{Cli, Command, ExtConfigAction, ExtensionAction, RoleAction};
use workspace::{SettingGetResult, SettingSetResult};

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
    let workspace_dir = args
        .workspace
        .unwrap_or_else(|| env::current_dir().expect("cannot determine current directory"));

    let app = UrApp::new(ur_root)?;
    let mut ws = app.open_workspace(&workspace_dir)?;

    match args.command {
        Command::Extension { action } => match action {
            ExtensionAction::List => {
                cli::print_list(ws.manifest());
            }
            ExtensionAction::Enable { id } => {
                ws.enable_extension(&id)?;
                println!("Enabled {id}");
            }
            ExtensionAction::Disable { id } => {
                ws.disable_extension(&id)?;
                println!("Disabled {id}");
            }
            ExtensionAction::Inspect { id } => {
                let entry = ws.find_extension(&id)?;
                cli::print_inspect(entry);
            }
            ExtensionAction::Config { id, action } => match action {
                ExtConfigAction::List { pattern } => {
                    let settings = ws.list_extension_settings(&id, pattern.as_deref())?;
                    println!("{:<40}{:<10}VALUE", "KEY", "TYPE");
                    for s in &settings {
                        println!("{:<40}{:<10}{}", s.key, s.type_name, s.value_display);
                    }
                }
                ExtConfigAction::Get { key } => match ws.get_extension_setting(&id, &key)? {
                    SettingGetResult::SecretSet => println!("****"),
                    SettingGetResult::SecretUnset => println!("(not set)"),
                    SettingGetResult::Value(v) => println!("{v}"),
                },
                ExtConfigAction::Set { key, value } => {
                    match ws.set_extension_setting(&id, &key, value.as_deref())? {
                        SettingSetResult::SecretRequired { name } => {
                            eprint!("{name}: ");
                            let secret = rpassword::read_password()?;
                            ws.store_secret(&id, &key, &secret)?;
                            println!("{key} stored securely.");
                        }
                        SettingSetResult::Stored { key: k, value: v } => {
                            println!("{id}: {k} = {v}");
                        }
                    }
                }
            },
        },
        Command::Role { action } => match action {
            RoleAction::List => {
                let roles = ws.list_roles()?;
                println!("{:<12}MODEL", "ROLE");
                for entry in &roles {
                    println!("{:<12}{}", entry.role, entry.model_ref);
                }
            }
            RoleAction::Get { role } => {
                let resolved = ws.resolve_role(&role)?;
                println!(
                    "{} -> {}/{}",
                    resolved.role, resolved.provider_id, resolved.model_id
                );
            }
            RoleAction::Set { role, model_ref } => {
                let resolved = ws.set_role(&role, &model_ref)?;
                println!(
                    "{} -> {}/{}",
                    resolved.role, resolved.provider_id, resolved.model_id
                );
            }
        },
        Command::Run => {
            turn::run(ws.engine(), ws.ur_root(), ws.workspace_path())?;
        }
    }

    Ok(())
}

/// Returns the user's home directory.
fn dirs_home() -> PathBuf {
    env::var("HOME").map(PathBuf::from).expect("HOME not set")
}
