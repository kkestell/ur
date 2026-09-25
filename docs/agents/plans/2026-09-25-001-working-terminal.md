# Working terminal

## Goal

Build milestone 0 in `docs/agents/todo.md`: the daemon runs a login shell through `portable-pty`,
and the GUI shows it in xterm.js. The daemon feeds all shell output through `vt100::Parser`, so
closing the GUI leaves the shell and its applications running, and reopening the GUI (at any window
size) restores the current screen and resumes live output. When this is done, the milestone 0 check
passes with `top` and an editor holding an unsaved buffer.

## Related code

There is no code yet. These references shape the work:

- `docs/agents/architecture.md` — Wire protocol, Project layout, Terminals in the daemon, GUI
  architecture, GUI state, and Transport define the pieces built here.
- `vt100-0.16.2/src/screen.rs` — `state_formatted()` writes the visible grid, cursor, and input
  modes (keypad, cursor keys, bracketed paste, mouse). It does not write the switch to the alternate
  screen; see Decisions.
- `vt100-0.16.2/src/screen.rs` — `set_size()` resizes both grids without reflow. The application
  redraws after it receives `SIGWINCH`.
- `portable-pty-0.9.0/src/cmdbuilder.rs` — `CommandBuilder::new_default_prog()` starts `$SHELL`
  (falling back to the password database) as a login shell with a `-` prefixed `argv0`.
- `tauri-2.11.5/src/ipc/channel.rs` — a `Channel<tauri::ipc::Response>` sends raw bytes, which
  arrive in the webview as an `ArrayBuffer` instead of a JSON array.

## Decisions

### Scope of the wire protocol

Milestone 0 adds only the requests it uses: `open_terminal`, `attach_terminal`, and
`terminal_resize`. `open_terminal` takes no workspace yet and starts the shell in `$HOME`; milestone
9 adds the workspace argument. There is no `detach_terminal` yet: closing the socket connection is
the only way to detach. The only event is `terminal_exited`.

### Frame layout

Tag `0x01` is `JSON` and `0x02` is `PTY`. A `PTY` payload is a big-endian `u32` terminal ID followed
by the raw bytes. The decoder rejects an unknown tag, a `PTY` payload shorter than four bytes, and
any payload over 16 MiB; the daemon closes that socket connection. `TerminalId` is a `u32` so ts-rs
emits `number`, not `bigint`.

### Terminal attachment order

`attach_terminal(terminal, rows, cols)` does this under the terminal's mutex: resize the PTY and the
parser if the size differs, take the screen snapshot, push it to the outbox as the attachment's
first `PTY` frame, and register the outbox. The response is sent after the lock is dropped, so the
snapshot frame arrives before the response. A daemon client therefore registers its receiver with
`Client::pty(id)` before it sends `attach_terminal`.

Resizing before the snapshot means the snapshot matches the size the xterm.js view already has. The
application then redraws at the new size through live output.

### Screen snapshot and the alternate screen

The screen snapshot is `state_formatted()`, prefixed with `ESC [ ? 1049 h` when `alternate_screen()`
is true. Without the prefix, a restored full-screen application draws into xterm.js's normal buffer,
and quitting it leaves its last frame on screen. With it, quitting returns to an empty normal buffer
and the shell redraws its prompt. The normal buffer's contents and scrollback from before attachment
are not restored; this is an accepted limitation.

The parser keeps no scrollback (`scrollback_len` 0) because the snapshot never includes it. A new
terminal starts at 24 rows and 80 columns until its first attachment.

### Shell exit

When the PTY read returns end-of-file or an error, the reader thread reaps the child, removes the
terminal, and sends `terminal_exited` to every attached outbox. `ur_client::Client` handles that
event by closing the terminal's `pty` receiver. The event and the output come over one socket in
order, so the receiver sees all output before it closes. The core then writes `[shell exited]` into
the xterm.js view. Reopening the GUI starts a new shell.

### Selection and reattachment

The core saves the terminal ID as the selection in the GUI state file. Its `attach_terminal` command
attaches the selected terminal. When there is no selection, or the daemon rejects the attachment
(for example after a daemon restart), it opens a new terminal, attaches it, and saves it as the
selection. The webview makes one call and does not know which case happened.

### Core commands and bindings

The core has three commands: `request` (a wire-protocol `Request` in, its `Response` out),
`attach_terminal(rows, cols, output)` which returns the terminal ID, and
`terminal_input(terminal, data)`. `attach_terminal` is a separate command because it takes a
`Channel`, which `request` cannot carry. The webview sends `terminal_resize` through `request`.

The core connects to the daemon on the first command and does not reconnect. If the daemon is not
running, the command fails and the webview shows the error. The reconnect loop belongs to
milestone 5.

ts-rs generates TypeScript for `protocol.rs` into `app/src/ipc/bindings/`, set by `TS_RS_EXPORT_DIR`
in `.cargo/config.toml`. `cargo test` regenerates them, and the generated files are committed so the
webview builds without Cargo.

### Input and outbox backpressure

The PTY writer has its own mutex, separate from the parser and outboxes, so a slow write never
blocks output. Input is written directly from the socket connection's task.

An outbox holds a bounded channel (1024 frames) and a `tokio_util::sync::CancellationToken` for its
socket connection. When `try_send` finds the channel full, it cancels the token, and the
connection's reader and writer end. A terminal drops an outbox when `try_send` reports it closed.

## Naming

- terminal, terminal ID, terminal attachment, screen snapshot, terminal attachment format, outbox,
  frame, request, event, daemon client, core, webview, Link, GUI state file, selection — as defined
  in `docs/agents/glossary.md`.
- `TerminalId` — The `u32` terminal ID the daemon assigns, counting from 1.
- `ClientMessage { id, request }` — The `JSON` frame payload from a daemon client: a request and its
  ID.
- `DaemonMessage` — The `JSON` frame payload from the daemon: `Response { id,
  response }` or
  `Event { event }`.
- `Response` — `Opened { terminal }`, `Done`, or `Error { message }`.
- `Event::TerminalExited { terminal }` — The shell of an attached terminal exited, and the daemon
  removed the terminal.
- `Terminals` — The daemon's map from `TerminalId` to terminal, and the next ID. It lives in
  `daemon/terminal.rs`.
- `Selection::Terminal(TerminalId)` — The selection in the GUI state file. Milestone 5 adds sessions
  to it.

## Test plan

Unit tests:

- `crates/ur-client/src/frame.rs`: `frames_round_trip` for a `JSON` frame and a `PTY` frame.
  `decoder_rejects_malformed_frames`, table-driven: unknown tag, `PTY` payload shorter than a
  terminal ID, payload over 16 MiB.
- `crates/ur/src/daemon/terminal.rs`: `snapshot_reproduces_the_screen`, table-driven over plain
  colored text with the cursor mid-line, and an active alternate screen. Each case feeds bytes to a
  parser, feeds its screen snapshot to a fresh parser of the same size, and compares
  `contents_formatted()`, cursor position, and `alternate_screen()`.

Integration tests in `crates/ur/tests/terminal.rs` run the `ur` binary with `CARGO_BIN_EXE_ur` in a
`tempfile` directory, with `UR_SOCKET`, `HOME`, and `SHELL=/bin/sh` set so no user shell
configuration runs. A helper connects with retry, and another reads `PTY` frames into a
`vt100::Parser` until the screen contains given text or five seconds pass.

- `attach_sends_the_screen_then_live_output`: one daemon client runs `echo one`; a second attaches,
  sees `one` in its first frame, then sees the output of `echo two`.
- `terminal_outlives_its_socket_connection`: open, attach, run `echo kept`, disconnect; a new daemon
  client attaches to the same ID, sees `kept`, and its input still runs.
- `attach_resizes_the_terminal`: reattach at 30 rows and 100 columns; `stty
  size` prints `30 100`.
- `attaching_an_unknown_terminal_is_an_error`.
- `shell_exit_ends_the_terminal`: after `exit`, the `pty` receiver closes and attaching the same ID
  is an error.

The GUI has no automated tests. The milestone 0 check in `docs/agents/todo.md` covers it.

## Implementation plan

### 1. One shell in xterm.js

- Root `Cargo.toml`: a workspace with members `crates/ur`, `crates/ur-client`, and `app/src-tauri`,
  and shared dependency versions. Add `node_modules/` and `app/dist/` to `.gitignore`.
- `crates/ur-client/src/frame.rs`: `Frame::Json(Bytes)` and
  `Frame::Pty { id: TerminalId, bytes: Bytes }`, with a `FrameCodec` that implements
  `tokio_util::codec` `Encoder` and `Decoder`.
- `crates/ur-client/src/protocol.rs`: `TerminalId`, `Request`, `Response`, `Event`, `ClientMessage`,
  and `DaemonMessage`, deriving serde and ts-rs. Add `socket_path()`: `$UR_SOCKET`, else
  `$TMPDIR/ur.sock`.
- `crates/ur-client/src/client.rs`: `Client::connect(path)`,
  `request(Request)
  -> io::Result<Response>`, `pty(id) -> mpsc::UnboundedReceiver<Bytes>`, and
  `pty_input(id, bytes)`. One writer task and one reader task; pending requests keyed by ID fail
  with an I/O error when the socket connection ends.
- `crates/ur/src/main.rs`: clap with the `daemon` subcommand.
- `crates/ur/src/daemon/mod.rs`: `start()` binds the socket. If the path exists and a connection
  succeeds, fail because a daemon is running; otherwise remove the stale file.
- `crates/ur/src/daemon/server.rs`: the accept loop, the per-connection reader and writer, and
  `Outbox`. Handle `open_terminal`, `attach_terminal`, `terminal_resize`, and `PTY` input. Log `PTY`
  input for an unknown terminal and drop it.
- `crates/ur/src/daemon/terminal.rs`: `Terminals` and one struct per terminal: the PTY master and
  attached outboxes under one mutex, and the writer under its own mutex. Attaching in this step
  sends only live output. Start `new_default_prog()` in `$HOME` with `TERM=xterm-256color` and
  `COLORTERM=truecolor`. The reader thread reads, fans bytes out, and handles shell exit.
- `app/`: scaffold with `pnpm create tauri-app` (React, TypeScript, Vite). Make `app/src-tauri` a
  binary-only crate with `main.rs`, drop the opener plugin, and add `@xterm/xterm` and
  `@xterm/addon-fit`.
- `app/src-tauri/src/link.rs`: `Link` owns the `ur_client::Client`, connected on first use. It
  starts a task per terminal attachment that forwards `pty` bytes to the attachment's
  `Channel<tauri::ipc::Response>` and writes `[shell exited]` when the receiver closes. The
  terminal-to-`Channel` map in `terminal.rs` waits until more than one terminal can be attached.
- `app/src-tauri/src/commands.rs`: `request`, `attach_terminal` (always opens a new terminal in this
  step), and `terminal_input`.
- `app/src-tauri/src/main.rs`: the builder, managed `Link`, and commands.
- `app/src/ipc/`: `request()`, `attachTerminal(rows, cols, onOutput)`, and `terminalInput()`, typed
  by the generated bindings.
- `app/src/components/TerminalPane.tsx`: open xterm.js with the fit addon, fit, attach at the fitted
  size, send `onData` to `terminalInput`, and on container resize fit again and send
  `terminal_resize`. `App.tsx` shows the pane full-window, or the error text when attaching fails.
- Add the frame tests.

### 2. Screen snapshot

- `crates/ur/src/daemon/terminal.rs`: add `vt100::Parser` to the terminal's mutex beside the PTY
  master and outboxes. The reader thread feeds it before fanning out. `terminal_resize` resizes the
  PTY and the parser together. Implement attachment in the order under Terminal attachment order,
  with the screen snapshot described above.
- Add `snapshot_reproduces_the_screen` and `attach_sends_the_screen_then_live_output`.
- Verify by hand with `top` running while the GUI is attached.

### 3. Reattachment

- `app/src-tauri/src/gui_state.rs`: read and write the GUI state file
  (`$XDG_STATE_HOME/ur/gui.json`, else `~/.local/state/ur/gui.json`), keyed by socket path, holding
  `Selection::Terminal`.
- `app/src-tauri/src/commands.rs`: `attach_terminal` follows Selection and reattachment.
- Add the remaining integration tests.
- Run the milestone 0 check in `docs/agents/todo.md`.

## Documentation updates

- `AGENTS.md`: map each added file. Add `pnpm -C app build` to Validation. If
  `cargo build --workspace` needs `app/dist` in a clean checkout, list the `pnpm` build first.
- `docs/agents/architecture.md`: under GUI architecture, list `attach_terminal` among the core
  commands and give the terminal `Channel` type as `Channel<tauri::ipc::Response>`. Under Terminals
  in the daemon, record the terminal attachment order and the alternate-screen prefix as the
  terminal attachment format. Under Wire protocol, add `terminal_exited`.
