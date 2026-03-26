use std::io::Write;
use std::path::Path;

use anyhow::Result;
use clap::Parser;
use mimalloc::MiMalloc;

use ur::app::UrApp;
use ur::cli::{self, Cli, Command, ExtensionAction, RoleAction};
use ur::logging;
use ur::session::{self, SessionEvent};
use ur::workspace::UrWorkspace;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

#[tokio::main]
async fn main() -> Result<()> {
    let args = Cli::parse();

    let ur_root = ur::resolve_ur_root();

    let log_handle = logging::init("ur", &ur_root, args.verbose, args.verbose)?;
    tracing::info!(
        verbose = args.verbose,
        ur_root = %ur_root.display(),
        "ur starting"
    );

    let result = run(&args, &ur_root).await;
    if let Err(ref e) = result {
        tracing::error!(error = %e, "ur exiting with error");
        eprintln!("log: {}", log_handle.path().display());
    }
    result
}

async fn run(args: &Cli, ur_root: &Path) -> Result<()> {
    let workspace_dir = args
        .workspace
        .clone()
        .unwrap_or_else(|| std::env::current_dir().expect("cannot determine current directory"));

    let app = UrApp::new(ur_root.to_owned())?;
    let mut ws = app.open_workspace(&workspace_dir)?;

    match &args.command {
        Command::Extension { action } => handle_extension(&mut ws, action),
        Command::Role { action } => handle_role(&mut ws, action).await,
        Command::Run { message } => handle_run(&mut ws, message).await,
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
            // Show tools and hooks if the extension is loaded.
            if let Some(ext) = ws.lua_extension(id) {
                let tools = ext.tool_descriptors();
                if !tools.is_empty() {
                    println!("tools:");
                    for t in &tools {
                        println!("  - {} \u{2014} {}", t.name, t.description);
                    }
                }
                let hooks = ext.hook_names();
                if !hooks.is_empty() {
                    println!("hooks:");
                    for h in &hooks {
                        println!("  - {h}");
                    }
                }
            }
        }
    }
    Ok(())
}

async fn handle_role(ws: &mut UrWorkspace, action: &RoleAction) -> Result<()> {
    match action {
        RoleAction::List => {
            let roles = ws.list_roles().await?;
            println!("{:<12}MODEL", "ROLE");
            for entry in &roles {
                println!("{:<12}{}", entry.role, entry.model_ref);
            }
        }
        RoleAction::Get { role } => {
            let resolved = ws.resolve_role(role).await?;
            println!(
                "{} -> {}/{}",
                resolved.role, resolved.provider_id, resolved.model_id
            );
        }
        RoleAction::Set { role, model_ref } => {
            let resolved = ws.set_role(role, model_ref).await?;
            println!(
                "{} -> {}/{}",
                resolved.role, resolved.provider_id, resolved.model_id
            );
        }
    }
    Ok(())
}

async fn handle_run(ws: &mut UrWorkspace, message: &str) -> Result<()> {
    let mut session = ws.open_session("demo")?;
    session
        .run_turn(message, |event| {
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
        })
        .await?;
    Ok(())
}
