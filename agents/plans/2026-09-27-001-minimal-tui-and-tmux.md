# Minimal TUI and tmux

## Goal

Replace the desktop app and daemon with a small Rust ACP client. Each `ur` process runs one session
against its own server. tmux supplies panes, shells, scrollback, copying, and process persistence
across terminal disconnects.

Follow `AGENTS.md`, `agents/code-style.md`, and `agents/testing.md`. Replace the affected decisions
in `agents/architecture.md` and definitions in `agents/glossary.md` as listed below.

## Related code

- `crates/ur/src/one_shot.rs` — direct ACP startup, prompting, permission options, and in-process
  tests provide the starting pattern.
- `crates/ur/src/daemon/acp.rs` — reuse the initialization and ACP version check only.
- `crates/ur/src/config.rs` — named executable selection and the config file location remain useful.
- `crates/ur-fake-server/src/lib.rs` — scripted replies, tool updates, concurrent permission
  requests, rejected prompts, and turn errors support testing without a model provider.

## Decisions

### Process ownership

Commit `a5de926` preserves the existing app. Delete the old implementation first; Git is the restore
mechanism. Do not retain an alternate GUI entry point or a dormant daemon.

`ur [--server NAME] [directory]` starts the interactive ACP client. The directory defaults to the
current directory and must exist. Keep named server selection: omit the name only when exactly one
server is configured. Each invocation launches a separate server and creates a new session.

Keep the existing config file location and reduce server entries to `name`, `command`, and `args`.
Remove IDs, icons, config writing, and the dependency on `ur-client`. Document replacing the old
config with this shape; add no migration. ur no longer reads or writes workspace or layout state.

The ACP client lives as long as its process. Detaching tmux leaves it running; quitting ur ends its
ACP connection and child process. A disconnected or failed server ends ur with a clear error after
restoring the terminal. There is no restart supervisor. Assume an already authenticated server that
supports text prompts without client filesystem or terminal capabilities.

This experiment creates new sessions only. It has no saved-history browser, resume command, image
input, config option picker, session list, notifications, or application-owned shell. Slash commands
are ordinary prompt text. The server remains responsible for any saved history it maintains.

### Terminal interaction

Use normal terminal scrollback, with no alternate screen and no full-screen redraw. Print replies as
text, leaving Markdown source intact. Print thoughts under a `Thinking` label, tool call titles and
status changes as lines, and supplied text or diff content as ordinary text. Merge partial tool
updates by tool call ID only to retain omitted fields; do not interpret tool names or raw input as
commands. Show a short marker for non-text content instead of rendering it.

Use crossterm for keyboard, paste, and resize events. One event loop owns terminal output and input
state. Its only editable area is a single input line after the printed output, shown when idle or
answering a permission request. Display the end of a long input buffer within the terminal width;
use `unicode-width` for display columns. Support typing, Backspace, Ctrl-U to clear, and Enter to
submit. A bracketed paste inserts text without submitting; preserve pasted newlines in the prompt
but show them as a visible marker in the input line. Do not implement a multiline editor or draft
history. Input for the next prompt is disabled while a turn is running.

ACP events continue to be processed while input is visible. Erase the input line, append any new
output, then redraw the input buffer. Escape terminal control characters in server-supplied text;
newlines and tabs remain readable. Restore raw mode, bracketed paste, and cursor state on ordinary
exit, errors, and panic. Keep server diagnostics from writing over the editable line.

Show permission requests oldest first, with supplied tool details and numbered option labels. Enter
submits the selected number, mapped to the exact supplied option ID. Invalid input leaves the
request pending. Ctrl-C during a turn sends `session/cancel` and resolves every pending permission
request as cancelled, including requests received while cancellation is in progress. Wait for the
prompt response before accepting another prompt. Ctrl-C when idle clears a nonempty input buffer and
exits when empty. Ctrl-D exits immediately, cancelling pending requests and closing the ACP
connection. Print a concise key reminder at startup.

Use the SDK dispatch pattern from the current daemon: notification and permission handlers enqueue
events and return promptly. They must not wait for user input. Prompt requests run without blocking
keyboard handling; the event loop allows only one prompt at a time. A rejected or failed prompt
prints its error and permits another prompt if the ACP connection is still open. Advertise no
optional client capabilities that this implementation does not provide.

The [crossterm event documentation](https://docs.rs/crossterm/latest/crossterm/event/index.html)
describes raw-mode keyboard input and the async event stream. Use that stream exclusively rather
than mixing it with blocking event reads.

### tmux

Add a repository `tmux.conf`, loaded explicitly on a separate tmux socket with
`tmux -L ur -f /absolute/path/to/tmux.conf new-session -A -s ur`. Do not overwrite the user's
personal tmux config. The [tmux manual](https://man.openbsd.org/tmux.1) documents config loading,
separate sockets, detach, and reattach.

Keep the default prefix and standard window, copy, and detach bindings. Add `|` and `-` for side and
stacked splits in the current pane's directory, enable mouse pane selection and resizing, and set a
generous fixed scrollback limit. Keep the status line small. Run `ur` or ordinary shell commands in
the resulting panes; no tmux plugins, ACP status polling, or Rust-to-tmux control layer.

## Naming

- **TUI** — the interactive terminal interface of the Rust ACP client.
- **Server**, **session**, **turn**, and **turn error** retain their ACP meanings from the glossary;
  update server ownership from the daemon to the ACP client.
- **Workspace** — the directory supplied when starting ur, no longer a persisted named entity.
- Use **tmux session**, **tmux server**, and **tmux client** with their qualifier to distinguish
  tmux's terms from ACP terms. Use **pane** for a tmux pane.
- Remove glossary terms belonging solely to the deleted GUI and wire protocol. Do not introduce a
  new name for terminal scrollback or an in-memory copy of it.

## Implementation plan

1. Remove the desktop app and daemon before building the replacement:
   - Delete `app/`, `crates/ur-client/`, `crates/ur/src/daemon/`, `crates/ur/src/one_shot.rs`, and
     `crates/ur/tests/terminal.rs`.
   - Delete `.cargo/config.toml`, `.github/workflows/release-macos.yml`, `scripts/build-sidecar`,
     `scripts/build-macos`, and `scripts/check-macos-package`.
   - In `Cargo.toml`, retain only `ur` and `ur-fake-server` as workspace members. Remove production
     dependencies used only by the GUI, daemon, wire protocol, or terminal hosting. Update
     `crates/ur/Cargo.toml` and regenerate `Cargo.lock` for the remaining code and new terminal
     dependencies. Recover the small useful ACP patterns from the restore commit as needed.
2. Rewrite `crates/ur/src/main.rs` for the interactive invocation, directory validation, and runtime
   lifetime. Simplify `crates/ur/src/config.rs` as specified above, keeping focused selection tests.
   Create `crates/ur/src/acp.rs` for initialization, session creation, prompt requests, notification
   delivery, permission responses, cancellation, and child lifetime. Retain ACP SDK types across
   this boundary instead of creating a replacement wire protocol.
3. Create `crates/ur/src/tui.rs` for terminal ownership, input state, output formatting, and the
   event loop. Keep terminal output serialization here, including diagnostics. Use no renderer
   framework or retained screen model. Put focused formatting and input tests beside this code, and
   ACP behavior tests beside `acp.rs`.
4. Update `crates/ur-fake-server/src/lib.rs` with a cancellable running-turn script and any minimal
   script needed for streamed replies or terminal input tests. Preserve useful existing test
   behavior. Correct daemon-specific comments there and in `crates/ur-fake-server/src/main.rs`.
5. Add `tmux.conf` and `crates/ur/tests/tui.rs`. The integration tests launch the built fake server
   and ur in an isolated tmux socket with temporary config and state. Mark the tmux tests ignored in
   ordinary Cargo runs; `make e2e` explicitly runs them. Use bounded waits and terminate only the
   tmux server created by each test.
6. Simplify `Makefile` to Rust and documentation checks, the tmux tests, interactive running, and a
   release binary build. Rewrite `scripts/run` to build and run ur, forwarding its arguments. Remove
   deleted-app entries from `.gitignore` and `dprint.json`. Do not add replacement packaging or a
   release publishing workflow for this experiment.

## Test plan

- ACP tests own: multiple prompts use one session; streamed updates keep their order; permission
  choices preserve server-supplied IDs; simultaneous requests remain independently answerable;
  cancellation resolves all pending requests and prevents a second prompt until the first ends; turn
  failures allow another prompt; server exit releases pending work and ends ur.
- Retain the initialization test for an unsupported ACP version. Test server selection and invalid
  directories before terminal mode changes or child launch.
- TUI unit tests own: partial tool updates retain omitted fields; control characters cannot execute
  terminal commands; paste is not submitted automatically; input width accounts for Unicode;
  resizing does not change the input's contents. Avoid snapshots of an entire screen.
- tmux tests own the terminal boundary: entering two prompts, reading a streamed reply, selecting a
  permission option, and cancelling a running turn work through actual keys. Cover narrow panes,
  resize, pasted text, and the return to a usable shell after normal exit and server failure.
- In a second tmux test, disconnect while ur waits for permission, reconnect, answer it, and submit
  another prompt. Confirm earlier output remains in scrollback. An adjacent shell remains usable.
- The deleted GUI, wire-protocol, daemon, and hosted-terminal tests lose their guarantees with those
  features. Direct ACP interaction moves from one-shot tests to `acp.rs`; terminal input behavior
  gains its owning tests in `tui.rs` and the tmux integration suite.
- Finally, try a configured real server in one pane beside a shell: prompt, approve, cancel, detach,
  and reattach. A missing or unauthenticated real server is reported as a skipped live check.

## Documentation updates

- `AGENTS.md` — replace the directory map and GUI-specific validation instructions.
- `README.md` — replace desktop installation and usage with building the binary, the reduced server
  config, terminal keys, and explicit tmux startup and reattachment. State that quitting ur or
  stopping tmux ends live work, while detaching tmux leaves it running.
- `agents/architecture.md` — replace the daemon and GUI architecture with ACP client and tmux
  ownership, server process lifetime, and the absence of local saved history.
- `agents/glossary.md` — apply the naming changes above.
- `agents/testing.md` — replace the GUI test discipline with the ACP and isolated tmux boundaries.
- `agents/code-style.md` — remove webview-only styling rules.
- `agents/todo.md` — replace the obsolete desktop task list with this experiment and its acceptance
  check. Leave historical plans, work logs, reviews, and issue rows intact.
