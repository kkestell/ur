# feat: Ratatui TUI first spike

## Related documents

- `docs/internal/brainstorms/2026-03-24-13-18-15-ratatui-tui-first-spike-brainstorm.md`

## Summary

Add a `ur-tui` binary that launches an interactive terminal UI using ratatui.
First spike scope: input bar, `/quit`, `/extensions` modal, no LLM turns.

## Prerequisites

The current codebase has no `src/lib.rs` — all modules are private to
`src/main.rs`. A second binary cannot access `UrApp`, `UrWorkspace`, etc.
without a library crate. This is the first step.

## Steps

### 1. Extract library crate (`src/lib.rs`)

Create `src/lib.rs` with the module declarations currently in `src/main.rs`.
Make modules `pub` so the TUI binary (and any future client) can import them.

**Create `src/lib.rs`:**
```rust
pub mod app;
pub mod cli;
pub mod config;
pub mod discovery;
pub mod extension_host;
pub mod extension_settings;
pub mod keyring;
pub mod manifest;
pub mod model;
pub mod provider;
pub mod session;
pub mod slot;
pub mod workspace;
```

**Update `src/main.rs`:** Remove all `mod` declarations. Change `use crate::`
imports to `use ur::`. The `main.rs` becomes a thin binary that depends on the
`ur` library crate. All existing behavior remains identical.

**Verify:** `make check && make test` pass. The `ur` binary works as before.

### 2. Add dependencies

```
cargo add ratatui crossterm
```

ratatui's default features include the crossterm backend. We also depend on
crossterm directly for terminal setup (`enable_raw_mode`,
`EnterAlternateScreen`, event polling).

### 3. Create `src/bin/ur-tui/main.rs` — terminal harness

Cargo auto-discovers `src/bin/ur-tui/main.rs` as a binary target named
`ur-tui`.

Responsibilities:
- Parse minimal CLI args (workspace path, verbose flag) — reuse clap or just
  use positional args
- Initialize `UrApp` and `UrWorkspace` (same pattern as `src/main.rs`)
- Call `setup_terminal()`: enable raw mode, enter alternate screen, create
  `ratatui::Terminal`
- Install a panic hook that restores terminal state before printing the panic
- Run the event loop (see step 4)
- Call `restore_terminal()` on clean exit

```rust
fn setup_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    Terminal::new(CrosstermBackend::new(stdout))
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}
```

The event loop:
```rust
loop {
    terminal.draw(|frame| ui::render(frame, &app))?;
    if crossterm::event::poll(Duration::from_millis(100))? {
        let event = crossterm::event::read()?;
        app.handle_event(event);
    }
    if app.should_quit {
        break;
    }
}
```

### 4. Create `src/bin/ur-tui/app.rs` — state and event handling

Core types:

```rust
pub struct App {
    pub mode: AppMode,
    pub input: String,
    pub cursor_pos: usize,
    pub messages: Vec<DisplayMessage>,
    pub workspace: UrWorkspace,
    pub should_quit: bool,
}

pub enum AppMode {
    Normal,
    Modal { title: String, content: String },
}

pub struct DisplayMessage {
    pub text: String,
}
```

Event handling:

- `handle_event(&mut self, event: Event)` — delegates to mode-specific handler
- **Normal mode key handling:**
  - Printable chars → insert into `input` at `cursor_pos`
  - Backspace → delete char before cursor
  - Delete → delete char at cursor
  - Left/Right → move cursor
  - Home/End → cursor to start/end
  - Enter → call `submit_input()`
  - Ctrl+C → set `should_quit = true`
- **Modal mode key handling:**
  - Escape → set mode back to `Normal`
  - All other keys → ignored (modal is read-only in this spike)
- `submit_input(&mut self)` — drain `input`, parse with `commands::parse()`,
  execute command, push result to `messages`

### 5. Create `src/bin/ur-tui/ui.rs` — rendering

**Main layout:** Vertical split using `Layout::vertical`:
- Top area: message log (fills available space)
- Bottom area: input bar (3 lines — border + 1 line of text)

**Message log:** A `Paragraph` widget wrapping all `DisplayMessage` texts,
joined by newlines. Auto-scroll to bottom (set scroll offset to keep latest
visible).

**Input bar:** A `Paragraph` inside a `Block` with a border. Show the current
`input` string. Set cursor position via `frame.set_cursor_position()`.

**Modal overlay:** When `AppMode::Modal`, render on top of the main layout:
1. Calculate a centered rect (60% width, 60% height)
2. `frame.render_widget(Clear, area)` to blank the overlay area
3. Render a `Block` with title and borders
4. Render a `Paragraph` with the modal content inside the block's inner area

```rust
pub fn render(frame: &mut Frame, app: &App) {
    let chunks = Layout::vertical([
        Constraint::Min(1),      // messages
        Constraint::Length(3),   // input
    ]).split(frame.area());

    render_messages(frame, app, chunks[0]);
    render_input(frame, app, chunks[1]);

    if let AppMode::Modal { title, content } = &app.mode {
        render_modal(frame, title, content);
    }
}
```

### 6. Create `src/bin/ur-tui/commands.rs` — slash command parsing

```rust
pub enum Command {
    Quit,
    Extensions,
    Unknown(String),
}

pub fn parse(input: &str) -> Option<Command> {
    let trimmed = input.trim();
    if !trimmed.starts_with('/') {
        return None;
    }
    match trimmed {
        "/quit" | "/q" => Some(Command::Quit),
        "/extensions" | "/ext" => Some(Command::Extensions),
        _ => Some(Command::Unknown(trimmed.to_owned())),
    }
}
```

**`/extensions` execution:** Reads `workspace.list_extensions()`, formats each
`ManifestEntry` as a line: `[enabled/disabled] id — name (slot)`. Returns the
formatted string, which `app.rs` wraps in a `Modal`.

**Non-slash input:** For this spike, non-slash text is echoed back to the
message log with a note like "(LLM turns not yet implemented)".

**Unknown slash commands:** Push an error message to the log:
"Unknown command: /foo"

### 7. Update Makefile

`cargo build` already discovers all binary targets, so `make build-ur` builds
both `ur` and `ur-tui` automatically. No Makefile changes needed for building.

For `make install`, add `ur-tui` to the install step:

```makefile
# After copying ur binary:
cp "$(HOST_BINARY_DIR)/ur-tui" "$(BINDIR)/ur-tui"
```

Where `HOST_BINARY_DIR` is `target/release` or `target/debug` depending on the
`DEBUG` flag.

### 8. Verify

- `make check` — both binaries compile cleanly
- `make clippy` — no warnings (pedantic lints apply to TUI code too)
- `make fmt` — TUI code is formatted
- Manual test: run `ur-tui`, type text, submit with Enter, run `/extensions`,
  press Escape, run `/quit`

## Design notes

- **No tokio.** crossterm's `poll()` is non-blocking. All slash commands in this
  spike are synchronous and instant.
- **Modal is one-at-a-time.** `AppMode` is an enum, not a stack. Sufficient for
  this spike. Can evolve to `Vec<Modal>` later.
- **The TUI binary shares all deps with `ur`.** ratatui/crossterm are compiled
  into the `ur` binary too even though it doesn't use them. Acceptable for a
  spike; split into a Cargo workspace if it becomes a concern.
- **Input editing is minimal.** Left/right cursor, backspace, delete, home/end.
  No history, no completion, no multi-line. Consider `tui-input` or
  `tui-textarea` crate in a future spike.

## Out of scope

- LLM turns / `run_turn()` integration
- Session persistence
- Message history with role indicators
- Multi-line input / text wrapping
- Scrollback in message area (auto-scroll only)
- Theming or color customization
- Terminal resize handling (ratatui handles basic reflow automatically)
