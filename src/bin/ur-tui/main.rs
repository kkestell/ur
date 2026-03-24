// Rust guideline compliant 2026-02-21

//! `ur-tui` — interactive terminal UI for the `ur` workspace assistant.
//!
//! First-spike scope: input bar, `/quit`, `/extensions` modal.
//! LLM turns are not implemented yet.

mod app;
mod commands;
mod ui;

use std::env;
use std::io::{self, Stdout};
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::poll;
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use mimalloc::MiMalloc;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use ur::app::UrApp;

use app::App;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

fn main() -> Result<()> {
    let ur_root = env::var("UR_ROOT").map_or_else(|_| dirs_home().join(".ur"), PathBuf::from);
    let workspace_dir = env::current_dir().expect("cannot determine current directory");

    let ur_app = UrApp::new(ur_root)?;
    let workspace = ur_app.open_workspace(&workspace_dir)?;

    let mut tui_app = App::new(workspace);

    let mut terminal = setup_terminal()?;

    // Restore the terminal on panic so the user's shell is not left in
    // raw mode if the TUI crashes.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Ignore errors during panic cleanup — the original panic message
        // is more important.
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        default_hook(info);
    }));

    let result = run_event_loop(&mut terminal, &mut tui_app);

    restore_terminal(&mut terminal)?;
    result
}

fn run_event_loop(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut App) -> Result<()> {
    loop {
        terminal.draw(|frame| ui::render(frame, app))?;

        // Poll with a short timeout so the loop stays responsive.
        // 100 ms is long enough to avoid busy-spinning while still
        // giving sub-frame latency for user keystrokes.
        if poll(Duration::from_millis(100))? {
            let event = crossterm::event::read()?;
            app.handle_event(&event);
        }

        if app.should_quit {
            break;
        }
    }
    Ok(())
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    Ok(Terminal::new(backend)?)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

/// Returns the user's home directory.
fn dirs_home() -> PathBuf {
    env::var("HOME").map(PathBuf::from).expect("HOME not set")
}
