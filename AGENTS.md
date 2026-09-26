THIS DOCUMENT MUST BE KEPT UP TO DATE

## Code

Map each file here as it is added, following the project layout in `docs/agents/architecture.md`.

- `Makefile` — `make check` runs every validation check; `make format` formats the Rust code and the
  Markdown files; `make e2e` runs the end-to-end suite; `make run` starts the daemon and the GUI for
  poking around.
- `scripts/run` — `make run`: writes the development config file when it is missing, builds and
  starts the daemon on the default socket, runs `pnpm tauri dev`, and stops the daemon when the GUI
  exits.
- `dprint.json` — the dprint config for the Markdown files, which wraps prose at 100 columns.
- `Cargo.toml` — the Cargo workspace and shared dependency versions.
- `.cargo/config.toml` — sets `TS_RS_EXPORT_DIR` so `cargo test` writes the TypeScript bindings to
  `app/src/ipc/bindings/`.
- `crates/ur-client/src/frame.rs` — `Frame` and `FrameCodec`, the wire protocol framing.
- `crates/ur-client/src/protocol.rs` — `TerminalId`, `Request` (with `open_terminal(workspace)`,
  `detach_terminal`, `close_terminal`, `delete_session`, and `set_config_option`), `Response`,
  `Event` (with `capabilities_changed`, `session_deleted`, `terminal_changed`, and
  `config_options_changed`), `Workspace`, `SessionSummary`, `TerminalSummary`, `Status`,
  `PendingPermission`, `Entry`, `ClientMessage`, `DaemonMessage`, `socket_path()`, and
  `state_dir()`.
- `crates/ur-client/src/client.rs` — `Client`, the daemon client used by the core and the CLI, with
  `request()`, `events()`, `pty()`, and `pty_input()`.
- `crates/ur-fake-server/` — the fake server: `lib.rs` exports `fake_server()`, `Hold`, and
  `SavedHistory`, and `main.rs` serves it over stdin and stdout, keeping its saved history in the
  file its argument names, if any. It advertises image prompts and, with saved history,
  `session/delete`, gives every session the `pace` config option, and answers
  `session/set_config_option` and `session/delete`.
- `crates/ur/src/main.rs` — the `ur` command line: `daemon`, `agent-run`, `workspace add|rm`, `ls`,
  `new`, `prompt`, `read`, `cancel`, `approve`, `deny`, and `wait`.
- `crates/ur/src/cli/` — `connect()`, `watch()`, and one file per CLI subcommand: `workspace.rs`,
  `ls.rs`, `new.rs`, `prompt.rs`, `read.rs`, `cancel.rs`, `answer.rs` (`approve` and `deny`), and
  `wait.rs`.
- `crates/ur/src/config.rs` — `Config` and `ServerConfig`, the config file.
- `crates/ur/src/one_shot.rs` — `ur agent-run`, the one-shot client, and its tests against a test
  agent.
- `crates/ur/src/daemon/mod.rs` — `start()`: binds the socket, removing a stale one, and reads the
  config file; `run()`: reads the state file, starts the supervisor, then serves.
- `crates/ur/src/daemon/state.rs` — `State`: the ACP connection with its capabilities and
  generation, workspaces, watchers, terminal summaries, and sessions, saved or loaded, with their
  transcripts, session titles, config options, operation guards and the loads and deletes they hold,
  statuses, unread flags, focus, pending permission requests, and subscribers.
- `crates/ur/src/daemon/state_file.rs` — `read()` and `write()` for the state file.
- `crates/ur/src/daemon/acp.rs` — `supervise()`: the supervisor, which starts the server again after
  it exits, and each ACP connection's handlers; `list_sessions()`; `initialize()`, shared with the
  one-shot client.
- `crates/ur/src/daemon/ops.rs` — `add_workspace()`, which also lists the workspace's saved
  sessions, and `remove_workspace()`, which also closes the workspace's terminals, which write the
  state file; `new_session()`, `subscribe()`, `prompt()`, `load()`, `delete_session()`, and
  `set_config_option()`, which handle the server's responses in `on_receiving_result` callbacks; and
  `cancel()`.
- `crates/ur/src/daemon/server.rs` — the accept loop, each socket connection's reader and writer,
  request handling, and `Outbox`.
- `crates/ur/src/daemon/terminal.rs` — `Terminals`: login shells through `portable-pty`, started in
  their workspace path, their `vt100::Parser` with `Title`, which records terminal title changes,
  terminal attachment and detaching, closing, the screen snapshot, and the reports to `State`.
- `crates/ur/src/daemon/tests.rs` — the daemon's session tests, run in process against the fake
  server.
- `crates/ur/tests/terminal.rs` — integration tests that run `ur daemon`.
- `app/src-tauri/Cargo.toml` — the `webdriver` feature, which embeds `tauri-plugin-wdio-webdriver`'s
  WebDriver server for the end-to-end suite. Release builds and `pnpm tauri dev` leave it out.
- `app/src-tauri/src/main.rs` — the Tauri builder, the dialog and opener plugins, managed `Link`,
  `setup`, which starts `Link::run()`, and the commands.
- `app/src-tauri/capabilities/default.json` — the main window's permissions: Tauri's core defaults,
  the dialog plugin's `open`, `ask`, and `message`, and the opener plugin's `open_url` for http,
  https, mailto, and tel URLs.
- `app/src-tauri/tauri.conf.json` — the app and window config. `dragDropEnabled` is off, so the
  webview receives HTML drop events with `File` objects.
- `app/src-tauri/src/link.rs` — `Link`: the reconnect loop `run()`, which owns the `Client`, replays
  the desired set `Desired { watch, subscribed, visible, focused }` after each connect, forwards
  events to the webview under the `watch`, `session`, and `connection` event names, routing each
  event type to one name, and forwards terminal output to the webview's `Channel`, one forwarding
  task per attached terminal, which `detach()` aborts before sending `detach_terminal`;
  `set_visible()` and `set_focused()`, which send `focus` for the visible sessions while the window
  has focus.
- `app/src-tauri/src/commands.rs` — the core commands `request`, `attach_terminal`,
  `detach_terminal`, `terminal_input`, `connection`, `selection`, `select`, and `set_visible`.
- `app/src-tauri/src/gui_state.rs` — the GUI state file: `Saved { selection }` per socket path,
  `Selection`, and `Connection`.
- `app/src/ipc/` — `request()`, `attachTerminal()`, `detachTerminal()`, `terminalInput()`,
  `connection()`, `selection()`, `select()`, `setVisible()`, and the `onWatch()`, `onSession()`, and
  `onConnection()` listeners; `bindings/` is generated by `cargo test` and committed.
- `app/src/store/watch.ts` — `WatchState`, `reduceWatch()`, `needsAttention()`,
  `workspaceSessions()`, `workspaceTerminals()`, `orderedWorkspaces()`, `attentionCount()`, and
  `useWatch()`: the connection, workspaces, sessions, terminals, and the server's capabilities
  outside React, with sessions ordered by attention and terminals in opening order.
- `app/src/store/sessions.ts` — every subscribed session's `ThreadState`, `useSession()`, which
  subscribes once, and `useThread()`.
- `app/src/transcript/blocks.ts` — `Block` and `ThreadState`, the thread's display types. The user
  block carries its images and the tool call block its tool call content; `ThreadState` also holds
  the slash commands, the latest usage, and the config options.
- `app/src/transcript/reduce.ts` — the transcript reducer `reduce()` and `applyEntry()`, the only
  webview code that reads `SessionUpdate` shapes.
- `app/src/transcript/permissions.ts` — `withPermissions()` and `Item`: the pending permission
  requests placed among the blocks, each merged with the tool call block of the same ID.
- `app/src/keys.ts` — `shortcutKind()` and `shortcutLabel()`, the shortcut for each permission
  option kind.
- `app/src/actions.ts` — `addWorkspace()`, `newSession()`, `newTerminal()`, `closeTerminal()`,
  `removeWorkspace()`, `deleteSession()`, `showWorkspaceMenu()`, `showSessionMenu()`, and
  `showTerminalMenu()`: the folder picker, confirmations, error messages, and native menus.
- `app/src/slash.ts` — `slashQuery()` and `matchingCommands()`, the command list's matching.
- `app/src/usage.ts` — `usageText()`, the usage indicator's popover lines.
- `app/src/components/Sidebar.tsx` — the header's `+`, the workspaces, their sessions and terminal
  rows, with each session's status mark, attention counts, and the workspace, session, and terminal
  menus.
- `app/src/components/Thread.tsx` — the items of the selected session, with user message thumbnails,
  and `Thought`, the Thinking row.
- `app/src/components/AgentMessage.tsx` — `AgentMessage`: an agent message rendered as Markdown,
  with links that open in the default browser, and the copy button.
- `app/src/components/ToolCall.tsx` — `ToolCall`, the Run Command block or the tool call row, and
  `toolIcon()`, the glyph for each tool kind.
- `app/src/components/ToolCallContent.tsx` — `ToolCallContentView`: tool call content as
  preformatted text.
- `app/src/components/Permission.tsx` — one pending permission request: its tool call title, its
  content through `ToolCallContentView`, one row per option, and "Awaiting Confirmation."
- `app/src/components/Editor.tsx` — `Editor` and `ImageAttachment`: the command list, image
  attachment chips, the prompt textarea, and the bottom row of the usage indicator, config pickers,
  and Send or Stop.
- `app/src/components/ConfigPicker.tsx` — `ConfigPicker`, the list or toggle for one config option.
- `app/src/components/UsageIndicator.tsx` — `UsageIndicator`, the ring and its popover.
- `app/src/components/TerminalPane.tsx` — `TerminalPane`: the terminal header and the xterm.js view
  of one terminal, which attaches on mount and detaches on unmount.
- `app/src/main.tsx` — renders `App`, and stops files dropped outside the editor from navigating the
  webview.
- `app/src/App.tsx` — the layout, the selection, which shows as no selection when its session or
  terminal is not in the watch state, the empty states with Add Workspace, the session header,
  `TerminalPane` keyed by terminal ID, `set_visible` for the rendered selection, and the permission
  shortcuts.
- `app/e2e/harness.ts` — `TestEnvironment`, `Gui`, and `e2eTest`, used by the end-to-end suite and
  ad-hoc checks. `TestEnvironment` writes a config file that launches the fake server with its saved
  history file and adds the `home` workspace; `Gui.showTerminal()` opens a terminal in it and
  selects its terminal row.
- `app/e2e/terminal.test.ts` — the end-to-end tests for terminals.
- `app/e2e/session.test.ts` — the end-to-end tests for agent sessions against the fake server.
- `app/e2e/attention.test.ts` — the end-to-end tests for session status, unread sessions, and
  permission requests.
- `app/e2e/editor.test.ts` — the end-to-end tests for the editor: slash commands, config pickers,
  the usage indicator, and image attachments.
- `app/e2e/thread.test.ts` — the end-to-end tests for thread rendering, against the fake server's
  `render` script, and for agent message links.

## Validation

For changes affecting behavior, interfaces, artifacts, or builds, run full validation with
`make check`: `cargo fmt --all -- --check`, `cargo test --workspace --all-targets --all-features`,
`cargo build --workspace --all-features`,
`cargo clippy --workspace --all-targets --all-features -- -D warnings`, `pnpm -C app build`,
`pnpm -C app test` (the webview's unit tests under vitest), and `dprint check`. Report any skipped
or failed check; do not call partial validation complete. For documentation-only, comment-only, and
filename-only changes, run `make check-docs` and use focused searches and diff inspection. Run
`make format-docs` to format the Markdown files.

A change to GUI behavior adds end-to-end tests for it against the fake server, extending the fake
server when it lacks the behavior, and runs the end-to-end suite, `make e2e`. Run it also when
finishing each top-level item in `docs/agents/todo.md`.

## Ox workflow

Plans, work logs, reviews, and issues live in `docs/agents/`.

- `/ox-plan` explores a change and writes a plan to `docs/agents/plans/`.
- `/ox-work` implements a plan, writes a work log to `docs/agents/work/`, and commits.
- `/ox-review` reviews code, writes a review to `docs/agents/reviews/`, and records each finding in
  `docs/agents/issues.csv`.
- `docs/agents/todo.md` is the task list. High and medium severity issues are added under the task
  they affect, or as new top-level items.
- `docs/agents/issues.csv` is the issue log. Each row has an id (`OX-NNNN`), a created time, a
  title, a severity (`low`, `medium`, `high`), the review lens that found it, a status (`unplanned`,
  `planned`, `wontfix`, `fixed`), and the review that found it. Issues found outside a review leave
  the lens and review empty. Append rows; never reorder or delete them, because `todo.md` links to
  rows by line number.

## Documentation

Never mention "milestones", "phases", etc. in code comments or documentation (other than todo.md) --
describe the work instead.

Read before planning and changing code:

- `docs/agents/architecture.md`
- `docs/agents/todo.md`
- `docs/agents/code-style.md`
- `docs/agents/glossary.md`
- `docs/agents/testing.md`

## Backwards Compatibility

Currently, there is none. Delete `state.json` and `gui.json` instead of adding migrations or
versions. Their directory is `$XDG_STATE_HOME/ur`, else `~/.local/state/ur`.

## Communication

- Always describe things directly, clearly, and plainly
- Follow big idea up front and progressive disclosure
- Never use jargon, invented terms, or shorthand
- Never mix definitions or overload terms
