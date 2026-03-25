use std::io::Write;
use std::path::Path;

use anyhow::Result;
use clap::Parser;
use mimalloc::MiMalloc;

use ur::app::UrApp;
use ur::cli::{self, Cli, Command, ExtConfigAction, ExtensionAction, RoleAction};
use ur::logging;
use ur::session::{self, SessionEvent};
use ur::workspace::{SettingGetResult, SettingSetResult, UrWorkspace};

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

fn main() -> Result<()> {
    let args = Cli::parse();

    let ur_root = ur::resolve_ur_root();

    let log_handle = logging::init("ur", &ur_root, args.verbose, args.verbose);
    tracing::info!(
        verbose = args.verbose,
        ur_root = %ur_root.display(),
        "ur starting"
    );

    let result = run(&args, &ur_root);
    if let Err(ref e) = result {
        tracing::error!(error = %e, "ur exiting with error");
        eprintln!("log: {}", log_handle.path().display());
    }
    result
}

fn run(args: &Cli, ur_root: &Path) -> Result<()> {
    let workspace_dir = args
        .workspace
        .clone()
        .unwrap_or_else(|| std::env::current_dir().expect("cannot determine current directory"));

    let app = UrApp::new(ur_root.to_owned())?;
    let mut ws = app.open_workspace(&workspace_dir)?;

    match &args.command {
        Command::Extension { action } => handle_extension(&mut ws, action),
        Command::Role { action } => handle_role(&mut ws, action),
        Command::Run { message } => handle_run(&mut ws, message),
    }
}

fn handle_extension(ws: &mut UrWorkspace, action: &ExtensionAction) -> Result<()> {
    match action {
        ExtensionAction::List => {
            cli::print_list(ws.manifest());
        }
        ExtensionAction::Enable { id } => {
            ws.enable_extension(id)?;
            println!("Enabled {id}");
        }
        ExtensionAction::Disable { id } => {
            ws.disable_extension(id)?;
            println!("Disabled {id}");
        }
        ExtensionAction::Inspect { id } => {
            let entry = ws.find_extension(id)?;
            cli::print_inspect(entry);
        }
        ExtensionAction::Config { id, action } => match action {
            ExtConfigAction::List { pattern } => {
                let settings = ws.list_extension_settings(id, pattern.as_deref())?;
                println!("{:<40}{:<10}VALUE", "KEY", "TYPE");
                for s in &settings {
                    println!("{:<40}{:<10}{}", s.key, s.type_name, s.value_display);
                }
            }
            ExtConfigAction::Get { key } => match ws.get_extension_setting(id, key)? {
                SettingGetResult::SecretSet => println!("****"),
                SettingGetResult::SecretUnset => println!("(not set)"),
                SettingGetResult::Value(v) => println!("{v}"),
            },
            ExtConfigAction::Set { key, value } => {
                match ws.set_extension_setting(id, key, value.as_deref())? {
                    SettingSetResult::SecretRequired { name } => {
                        eprint!("{name}: ");
                        let secret = rpassword::read_password()?;
                        ws.store_secret(id, key, &secret)?;
                        println!("{key} stored securely.");
                    }
                    SettingSetResult::Stored { key: k, value: v } => {
                        println!("{id}: {k} = {v}");
                    }
                }
            }
        },
    }
    Ok(())
}

fn handle_role(ws: &mut UrWorkspace, action: &RoleAction) -> Result<()> {
    match action {
        RoleAction::List => {
            let roles = ws.list_roles()?;
            println!("{:<12}MODEL", "ROLE");
            for entry in &roles {
                println!("{:<12}{}", entry.role, entry.model_ref);
            }
        }
        RoleAction::Get { role } => {
            let resolved = ws.resolve_role(role)?;
            println!(
                "{} -> {}/{}",
                resolved.role, resolved.provider_id, resolved.model_id
            );
        }
        RoleAction::Set { role, model_ref } => {
            let resolved = ws.set_role(role, model_ref)?;
            println!(
                "{} -> {}/{}",
                resolved.role, resolved.provider_id, resolved.model_id
            );
        }
    }
    Ok(())
}

fn handle_run(ws: &mut UrWorkspace, message: &str) -> Result<()> {
    let mut session = ws.open_session("demo")?;
    session.run_turn(message, |event| {
        match event {
            SessionEvent::TextDelta(delta) => {
                print!("{delta}");
                let _ = std::io::stdout().flush();
            }
            SessionEvent::AssistantMessage { .. } => {
                println!();
            }
            SessionEvent::ApprovalRequired { .. } => {
                return Some(session::ApprovalDecision::Approve);
            }
            _ => {}
        }
        None
    })?;
    Ok(())
}
