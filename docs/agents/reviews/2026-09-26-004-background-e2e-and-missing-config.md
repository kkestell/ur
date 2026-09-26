# Background end-to-end app and missing config file

## Scope and coverage

Reviewed the uncommitted working-tree change: the daemon starting without a config file
(`crates/ur/src/config.rs`, `crates/ur/src/daemon/mod.rs`, `crates/ur/src/one_shot.rs`), `make run`
no longer writing a development config and stopping when another daemon listens (`scripts/run`,
`Makefile`), the `webdriver` feature keeping the app in the background on macOS
(`app/src-tauri/src/background.rs`, `app/src-tauri/src/main.rs`, `app/src-tauri/Cargo.toml`, the
workspace `Cargo.toml`), the end-to-end harness and tests dropping `Gui.focusWindow()`, and the
matching `AGENTS.md`, `docs/agents/testing.md`, and `docs/agents/architecture.md` text. Traced the
missing-config path through `State::new()` and `App.tsx`'s `showSetup`, which opens the server setup
screen while `server_state.command` is null, and the focus path through `Link`'s initial
`focused: true`. Checked Tauri 2.11.6's `set_activation_policy`, which after launch posts to the
event loop, so the direct `setActivationPolicy:` call in `show_main_window()` is needed to switch
policy before `orderFrontRegardless`. Used the correctness, simplicity, comments, documentation, and
error-handling lenses.

The end-to-end suite was run in full with the app in the background, which exercised the background
window, the server setup screen without a config file, and the tests that rely on the window's
initial focus.

## Fixed

- **Repeated profanity in the module comment** (`app/src-tauri/src/background.rs:4`): five identical
  `//! FILTHY FUCKING HACK` lines followed the module summary. They told the reader nothing the
  summary and the `show_main_window()` comments do not, and appeared in the generated module
  documentation. Deleted them.
- **Project layout said the daemon always starts the supervisor**
  (`docs/agents/architecture.md:252`): the `mod.rs` line said `start()` starts the supervisor and
  waits for initialize, but without a config file it now serves at once and waits for a server
  choice. The line now says it does so when the config names a server.

## Findings

No open findings.

## Checks run

- `make check` — passed (`cargo fmt`, `cargo test`, `cargo build`, `cargo clippy`, `pnpm build`,
  `pnpm test`, `dprint check`), before the fixes.
- After the fixes: `cargo fmt --all -- --check`, `cargo clippy -p ur-app --all-features`, and
  `dprint check` — passed.
- `make e2e` — passed, 77 tests, with the app kept in the background on macOS.

## Verdict

The change is correct: a missing config file now opens the server setup screen without an error,
`agent-run` still names the missing file, and the background mode keeps the GUI's initial window
focus, which the tests that dropped `focusWindow()` rely on. Two small fixes were made to a comment
and the architecture doc. The end-to-end suite passes with the app in the background.
