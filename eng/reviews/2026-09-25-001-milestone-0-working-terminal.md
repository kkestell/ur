# Milestone 0 working terminal review

## Scope and coverage

Reviewed the uncommitted milestone 0 work against
`eng/plans/2026-09-25-001-working-terminal.md`, milestone 0 in `eng/todo.md`,
and `eng/architecture.md`. I read `eng/work/2026-09-25-001-working-terminal.md`
for the departures from the plan and the manual verification. The code:

- `crates/ur-client/` (framing, protocol types, `Client`).
- `crates/ur/` (daemon socket, server, terminals, integration tests).
- `app/src-tauri/src/` (`Link`, commands, GUI state file).
- `app/src/` (IPC wrappers, `TerminalPane`, `App`, generated bindings).
- The workspace manifests and the documentation changes in `AGENTS.md` and
  `eng/architecture.md`.

Lenses: correctness, concurrency, resources, error handling, testing,
documentation, Rust idioms, and Rust Cargo.

Gaps:

- I did not run the GUI. The work log records the milestone 0 check passing
  through an embedded WebDriver server: `top` and vim with an unsaved buffer
  both survived reopening at a smaller and a larger window size. That check
  typed by dispatching `insertText` events. The user's manual pass under
  Checks run covered real key presses.
- The `webdriver` feature and `tauri-plugin-wdio-webdriver` belong to
  `eng/plans/2026-09-25-002-end-to-end-suite.md` and were not reviewed.
- Scaffolded files (icons, `tsconfig*.json`, `vite.config.ts`, lockfiles) were
  checked only for their effect on the build.

## Findings

The daemon, `ur-client`, and core follow the plan's decisions, with the
departures the work log records:

- Terminal attachment order.
- The alternate-screen prefix.
- Shell exit.
- Selection and reattachment.
- Outbox backpressure.

Each test in the plan exists and passes. Lock order in `Terminals` has no
cycle. The map lock held in `attach` does guarantee that a registered outbox
receives `terminal_exited`.

### Low

#### Documentation

- **`AGENTS.md` does not map every added file** (`AGENTS.md:7`): The section
  says to map each file as it is added, and it maps configuration such as
  `.cargo/config.toml`. Several added files are missing:
  - `crates/ur-client/src/lib.rs`.
  - `app/src/main.tsx`, which records why StrictMode is off.
  - `app/src-tauri/tauri.conf.json`.
  - `app/src-tauri/capabilities/default.json`.

  A reader following the map will not find where the window, the
  frontend build, or the IPC permissions are configured. Add one line for each.

## Unresolved questions

- **Terminal input and resizes may reach the daemon out of order**
  (`app/src-tauri/src/commands.rs:47`, `app/src/components/TerminalPane.tsx:21`,
  `app/src/components/TerminalPane.tsx:37`): `terminal_input` and `request` are
  async Tauri commands. Tauri spawns a separate task for each call to an async
  command (`tauri-2.11.5/src/ipc/mod.rs:324-329`), and the runtime has several
  worker threads. Two back-to-back `onData` calls can therefore send their `PTY`
  frames in either order. xterm.js makes such calls when it answers an
  application's terminal queries. Two back-to-back resizes can also leave the
  PTY at the older size. I did not reproduce either case. To confirm, send
  numbered input in a tight loop from the webview and check the order the shell
  receives it. If input is reordered, make `terminal_input` a synchronous
  command so each call writes to `Client` in the order it arrives.
- **The screen snapshot does not restore the scroll region or origin mode**
  (`crates/ur/src/daemon/terminal.rs:180`): `state_formatted()` writes only the
  grid, cursor, attributes, and input modes
  (`vt100-0.16.2/src/screen.rs:224-229`, `vt100-0.16.2/src/screen.rs:385-406`).
  After a reattachment at the same size, no `SIGWINCH` arrives, so an
  application that set a scroll region earlier does not redraw. Its later
  scrolling would then move the whole screen. To confirm, reattach at the same
  size to an application that keeps a scroll region set, such as a progress bar
  pinned to the bottom row, and watch its next output.
- **The GUI buffers output without limit when output outpaces xterm.js**
  (`crates/ur-client/src/client.rs:89`, `app/src-tauri/src/link.rs:55-62`):
  The core reads the socket into an unbounded `pty` channel and forwards each
  chunk to the `Channel` at once. The daemon therefore never sees the GUI fall
  behind, and a slow webview queues output in the core and the webview instead.
  To confirm, run `yes` in the GUI for a few seconds, press Ctrl-C, and measure
  how long the prompt takes to appear and how much memory the app uses.

## Checks run

- `cargo fmt --all -- --check`: passed.
- `cargo test --workspace --all-targets --all-features`: passed.
  - `ur`: 1 unit test and 5 integration tests.
  - `ur-client`: 2 frame tests and 3 binding exports.
- `cargo build --workspace --all-features`: passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`:
  passed.
- `pnpm -C app build`: passed. Vite warned that the bundle is larger than
  500 kB.
- A Python check against the built daemon, using the socket directly:
  - It wrote 200 KB of input to a terminal while a loop printed output.
  - Output kept arriving on both attached socket connections.
  - A `terminal_resize` on the writing connection answered in 20 ms.
  - This ruled out input writes blocking the connection's output on macOS for
    that case.
- Read the Tauri, vt100, and portable-pty sources cited above. The frontend
  directory check in `tauri-codegen` is skipped for a development build with
  `devUrl`, so `cargo build` does not need `app/dist` first. The
  `@tauri-apps/api` `Channel` delivers messages to the webview in order.
- A manual pass by the user in the running GUI, with a real keyboard:
  - Line editing, history, tab completion, Ctrl-C, and live resizing.
  - `top` and vim with an unsaved buffer across closing and reopening at a
    different size.
  - Shell exit.

  The user reported everything working. On reopening at a larger size, the
  old screen shows briefly at the old size until the application redraws.
  The plan accepts this, because the snapshot cannot reflow.

## Verdict

Milestone 0 matches its plan and the recorded departures. The automated
validation passes, and the work log records the milestone 0 check passing. The
only confirmed finding is a documentation gap. The first unresolved question,
about input order, is worth confirming.
