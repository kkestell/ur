# Ratatui TUI — First Spike

## How Might We

How might we provide an interactive, persistent terminal interface for Ur that
makes the existing UrApp/UrWorkspace/UrSession abstractions feel like a live,
conversational environment?

## Why This Approach

Dual motivation: (1) build a real daily-driver interactive client for Ur, and
(2) validate that the UrApp/UrWorkspace/UrSession abstractions work well for
rich, non-CLI consumers. The TUI is both the product and the test.

## Scope — First Spike

Deliberately narrow. Slash commands only, no LLM turns:

1. **Separate `ur-tui` binary** — second binary target in the same crate
   (`src/bin/ur-tui/main.rs`). Shares the `ur` library code.
2. **Input bar** — minimal styled text input at the bottom of the screen.
   Accepts free text and slash commands.
3. **`/quit`** — exits the TUI cleanly (restores terminal state).
4. **Modal overlay** — a dismissable (Escape) panel that renders on top of the
   main view. Reusable for future features (extension management, settings,
   help).
5. **`/extensions`** — first modal demo. Displays the list of loaded extensions
   with their enabled/disabled status via `UrWorkspace::list_extensions()`.

What's explicitly **out of scope**: LLM interaction (`run_turn`), session
persistence, message history rendering, multi-line input, theming/styling.

## Architecture

**TEA (The Elm Architecture) with module separation** — idiomatic ratatui:

```
src/bin/ur-tui/
  main.rs       # terminal setup, run loop, teardown
  app.rs        # App state, AppMode enum, handle_event(), handle_command()
  ui.rs         # render functions: draw_main, draw_input, draw_modal
  commands.rs   # parse slash commands, dispatch to UrWorkspace methods
```

Core types:

```rust
struct App {
    mode: AppMode,
    input: String,
    cursor_pos: usize,
    workspace: UrWorkspace,
    messages: Vec<DisplayMessage>,  // simple log of command output
    should_quit: bool,
}

enum AppMode {
    Normal,
    Modal { title: String, content: String },
}
```

**Event loop:** Standard ratatui loop — `crossterm::event::poll()` with a tick
rate, then `terminal.draw(|f| ui::render(f, &app))`, then
`app.handle_event(event)`.

## Async Strategy

**Sync-first.** No tokio. No async/await.

- `crossterm::event::poll()` is already non-blocking — sufficient for TUI event
  handling.
- All slash commands in this spike are instant (read-only workspace queries).
- When LLM turns are added later: `std::thread::spawn` for `run_turn()` +
  `std::sync::mpsc` channel for `SessionEvent` streaming back to the TUI
  thread.
- Async in the Rust host buys almost nothing — the expensive I/O (HTTP to LLM
  APIs) happens inside WASM extensions, on the other side of the wasmtime
  boundary.

## Key Decisions

1. **Separate binary, same crate** — `src/bin/ur-tui/main.rs`. Avoids workspace
   restructuring. Trade-off: TUI deps (ratatui, crossterm) are linked by the
   `ur` binary too, but the cost is negligible for a spike.
2. **TEA pattern** — state/update/view separation. Enough structure to be clean,
   not so much it's over-engineered.
3. **Modal as AppMode variant** — simple enum-based mode switching. No component
   traits, no trait objects. One modal at a time.
4. **Slash commands only** — no LLM turns. Validates the TUI shell and the
   UrWorkspace read API. LLM integration is the next spike.
5. **No tokio** — sync crossterm polling. Background threads + channels when
   needed later.

## Validated Assumptions

1. Separate `ur-tui` binary target in the same crate
2. First spike is slash commands only, no LLM turns
3. `/extensions` as the modal demo, `/quit` to exit
4. Sync-first: crossterm polling, no tokio
5. Both daily-driver and abstraction-validation goals
6. ratatui + crossterm as the TUI framework
7. No terminal size or timeline constraints
8. Greenfield — refactor freely

## Constraints

- No timeline pressure.
- WASM boundary is synchronous — async in the host doesn't help.
- ratatui is the chosen framework (not cursive, not tui-rs).
- Must work with the existing UrApp → UrWorkspace → UrSession hierarchy.

## Failure Modes

- **Binary bloat:** TUI deps inflate the `ur` CLI binary. Mitigated: split into
  workspace crate if it becomes a problem. Negligible for a spike.
- **Modal doesn't compose:** Single-modal enum might not scale to nested modals
  or modal-with-input. Mitigated: for this spike, one modal at a time is
  sufficient. Can evolve to a modal stack later.
- **Input editing feels bad:** Minimal text input without readline-like features
  (history, cursor movement) can be frustrating. Mitigated: this is a spike;
  basic left/right cursor + backspace is enough. Consider `tui-input` or
  `tui-textarea` crate later.

## Open Questions

- Should the message/log area support scrolling in this spike, or is that
  deferred?
- Should `/extensions` be read-only in the modal, or should it support toggling
  enable/disable interactively? (Likely deferred.)
- Will we want a command palette (fuzzy-find slash commands) eventually?
- How should the TUI handle terminal resize events?

## Dependencies to Add

- `ratatui` (includes crossterm by default)
- `crossterm` (explicit, for raw-mode terminal setup)

## SCAMPER Insights

- **Combine:** The separate CLI subcommands (`extension list`, `role list`,
  `run`) become unified slash commands in one persistent session.
- **Adapt:** The `SessionEvent` callback pattern maps naturally to TUI widget
  updates when LLM turns are added.
- **Reverse:** Instead of "launch, run one command, exit" (CLI), the TUI keeps
  state alive across interactions.
