# Workspace terminal controls

## Goal

Build the Workspace terminal controls section of `docs/agents/todo.md`. Today the GUI has one
terminal, reached from the Terminal row below the workspaces and saved in the GUI state file. The
daemon's terminals belong to no workspace, start in `$HOME`, have no terminal title, and cannot be
closed. When this is done:

- `open_terminal(workspace)` starts a login shell in the workspace path. `watch` lists every
  terminal with its workspace and terminal title, which is the shell's name until a program sets
  one.
- In the sidebar, each workspace lists its terminals after its sessions, each row showing a terminal
  icon and its terminal title, with no status. The global Terminal row is gone.
- The workspace menu is New Session, New Terminal, a separator, and Remove Workspace…. New Terminal
  opens a terminal and selects it.
- Selecting a terminal shows the terminal header, with the icon and terminal title, above its
  xterm.js view. Selecting another terminal, or a session, detaches the one shown.
- Right-clicking a terminal row opens the terminal menu with Close Terminal, which stops the
  terminal's shell and the programs running in it. The terminal leaves the sidebar when its shell
  exits, whether it was closed or exited by itself.
- Remove Workspace stops the workspace's terminals too, and its confirmation says so.

## Related code

- `crates/ur/src/daemon/terminal.rs` — `Terminals`: `open()`, `attach()`, `resize()`, `write()`, and
  the reader thread `read()`, which removes the terminal and sends `terminal_exited` to the attached
  outboxes when the shell exits.
- `crates/ur/src/daemon/state.rs` — `State::watch()` builds the watch snapshot;
  `remove_workspace()`, `broadcast()`, and `publish()` are the patterns for the terminal methods.
- `crates/ur/src/daemon/ops.rs` — `remove_workspace()`.
- `crates/ur/src/daemon/server.rs` — `handle()` dispatches the terminal requests.
- `crates/ur/src/daemon/mod.rs` — `run()` passes `Arc::default()` as the `Terminals`.
- `crates/ur-client/src/protocol.rs` — `Request::OpenTerminal`, `Event::TerminalExited`,
  `WatchSnapshot`, and `SessionSummary`, the pattern for `TerminalSummary`.
- `crates/ur-client/src/client.rs` — `Routes::deliver()` closes a terminal's `pty` receiver on
  `terminal_exited`, and drops a route whose receiver is gone.
- `crates/ur/tests/terminal.rs` — the terminal integration tests against `ur daemon`.
- `crates/ur/src/daemon/tests.rs` — `watch()` destructures `WatchSnapshot`; the test near line 966
  sends `OpenTerminal`.
- `app/src-tauri/src/link.rs` — `attach()` and its forwarding task, and `run()`, which skips
  `terminal_exited`.
- `app/src-tauri/src/commands.rs` — `attach_terminal`, which opens a terminal when the GUI state
  file has none.
- `app/src-tauri/src/gui_state.rs` — `Saved { terminal, selection }` and `Selection::Terminal`.
- `app/src/store/watch.ts` — `reduceWatch()` and `workspaceSessions()`.
- `app/src/actions.ts` — `newSession()`, `removeWorkspace()`, `showWorkspaceMenu()`, and
  `showSessionMenu()`.
- `app/src/components/Sidebar.tsx`, `TerminalPane.tsx`, and `app/src/App.tsx` — the Terminal row,
  the xterm.js view, and the selection.
- `app/e2e/harness.ts` — `Gui.showTerminal()` clicks the Terminal row, and `newSession()` adds the
  `home` workspace.
- `docs/agents/wireframes/terminal.png` — the sidebar rows, the terminal header, and both menus.
- `portable-pty` 0.9.0 — `CommandBuilder::cwd()`, `CommandBuilder::get_shell()`, and
  `Child::clone_killer()`, whose killer sends `SIGHUP` without waiting.
- `vt100` 0.16.2 — `Callbacks::set_window_title()`, which OSC 0 and OSC 2 call, and
  `Parser::new_with_callbacks()`.

## Decisions

### Wire protocol

Request changes:

```rust
/// Starts a login shell in the workspace path.
OpenTerminal { workspace: String },
/// Stops sending the terminal's output to this socket connection. The
/// terminal keeps running.
DetachTerminal { terminal: TerminalId },
/// Sends `SIGHUP` to the terminal's shell. The terminal is removed, and
/// `terminal_exited` sent, when the shell exits.
CloseTerminal { terminal: TerminalId },
```

`open_terminal` still answers `Opened { terminal }`. A request naming an unknown terminal or
workspace answers `Error`.

```rust
/// One terminal as watch shows it.
pub struct TerminalSummary {
    pub terminal: TerminalId,
    pub workspace: String,
    pub title: String,
}
```

Event changes:

- `WatchSnapshot` gains `terminals: Vec<TerminalSummary>`, in the order they were opened.
- `TerminalChanged { summary: TerminalSummary }`, a watch event: a terminal was opened, or its
  terminal title changed.
- `TerminalExited { terminal }` now goes to every watching socket connection as well as every
  attached one, once per socket connection, after all of the terminal's output. One event serves
  both, because `Client` already ends the `pty` receiver on it and the sidebar needs the same fact.
- `WorkspaceRemoved` also removes the workspace's terminals.

The architecture's request list already names `open_terminal(workspace)`, `detach_terminal`, and
`close_terminal`, so these follow it.

### Terminal summaries live in `State`

`State` gains `terminals: Vec<TerminalSummary>`, so the watch snapshot and watch events are built
under one lock like sessions. `Terminals` keeps the PTYs, parsers, and attachments under their own
mutexes, as Terminals in the daemon describes, and holds an `Arc<Mutex<State>>` to report to it.
`Terminals::default()` becomes `Terminals::new(state)`.

Lock order is `State`, then the terminal map, then a terminal's `output`. No code takes `State`
while holding the other two:

- `Terminals::open(workspace)` locks `State` for the whole open: it looks up the workspace path,
  spawns the shell, inserts the terminal in the map, calls `State::add_terminal()`, and only then
  starts the reader thread. Holding the lock keeps the workspace from being removed between the
  lookup and the add, and the reader's first title change waits until the summary exists.
- The reader thread takes the changed terminal title while it holds `output`, releases `output`,
  then calls `State::set_terminal_title()`.
- When the shell exits, the reader removes the terminal from the map, takes its attached outboxes,
  releases both locks, then calls `State::remove_terminal(id, &attached)`.

`State` methods:

- `add_terminal(summary)` pushes the summary and broadcasts `TerminalChanged`.
- `set_terminal_title(id, title)` broadcasts `TerminalChanged` only when the terminal title differs.
  A terminal removed with its workspace is no longer there, and nothing happens.
- `remove_terminal(id, attached)` removes the summary if it is still there, broadcasts
  `TerminalExited` to the watchers, and sends it to each attached outbox whose socket connection is
  not among the watchers (`same_connection`).
- `remove_workspace()` also removes the workspace's terminal summaries and returns their IDs, so its
  return type becomes `anyhow::Result<Vec<TerminalId>>`. `ops::remove_workspace()` takes the
  `Terminals` and calls `Terminals::close()` for each ID after the `State` lock is released.

### Terminal title

`Output.parser` becomes `vt100::Parser<Title>`, where `Title { changed: Option<String> }` implements
`Callbacks::set_window_title()` by storing `String::from_utf8_lossy(title)`. After each `process()`,
the reader takes `changed`. The first terminal title is the file name of
`CommandBuilder::get_shell()`, read before spawning, so `/bin/sh` shows as `sh`. OSC 1, the icon
name, is ignored.

### Closing a terminal

`Terminal` gains `killer: Mutex<Box<dyn ChildKiller + Send + Sync>>` from `child.clone_killer()`.
`Terminals::close(id)` calls `kill()`, which sends `SIGHUP` to the shell. The shell passes it to its
jobs, and the kernel sends it to the foreground process group when the shell exits, so `npm run dev`
stops too. The reader thread then sees the PTY close and removes the terminal through the existing
exit path, so a closed terminal and a shell that ran `exit` end the same way. A program that ignores
`SIGHUP` and keeps the PTY open keeps its terminal listed; that is an explicit limitation. Close
Terminal has no confirmation, matching the menu label without an ellipsis in the wireframe.

### Detaching

`Terminals::detach(id, outbox)` removes the outbox of that socket connection from the terminal's
attached outboxes. In the core, `Link` gains `attachments: Mutex<HashMap<TerminalId, AbortHandle>>`
for the forwarding tasks. `Link::attach()` records the task's handle; attaching the same terminal
again already closes the earlier `pty` receiver, which ends the earlier task. `Link::detach()`
aborts the task, which drops the receiver so `Client` drops the route, and sends `detach_terminal`.
The forwarding task no longer writes `[shell exited]`: when a terminal exits, `terminal_exited`
removes it from the sidebar and its view unmounts, and when the daemon disconnects, the whole window
shows the no-connection state.

### GUI

- `Selection::Terminal` becomes `Terminal { terminal: TerminalId }`, and `Saved` drops `terminal`.
  Delete `gui.json` after this change.
- The `attach_terminal` command takes the terminal ID with the size and only attaches; opening is a
  `request` with `open_terminal`. A new `detach_terminal` command calls `Link::detach()`.
- `TerminalPane` takes the `TerminalSummary`, renders the terminal header above the xterm.js view,
  and keeps its own attach error. `App` renders it keyed by terminal ID, so switching terminals
  mounts a fresh xterm.js view that attaches and gets the screen snapshot; its effect cleanup calls
  `detachTerminal()`.
- A selected terminal that is not in the watch state shows as no selection, as a missing session
  does. This covers the moment between `open_terminal`'s response and its `terminal_changed`, and a
  terminal that exited.
- The terminal icon is the text `>_` in a monospace span, in the sidebar row and the terminal
  header.
- The Link routes `terminal_changed` and `terminal_exited` to the `watch` event name.

### End-to-end harness

`TestEnvironment.start()` adds a workspace named `home` at the test's `HOME` with
`ur workspace add`, and `newSession()` only creates a session in it. `Gui.showTerminal()`, when no
terminal is shown, opens one through `Gui.request()`, since WebDriver cannot drive native menus,
then clicks its terminal row. A reopened GUI restores the terminal selection and needs neither step.
The empty states test removes `home` to reach No workspaces, and the test of a session that comes
back with its workspace waits for no row under any workspace. The other end-to-end guarantees are
unchanged.

## Naming

- **Terminal summary** — one terminal as watch shows it: its terminal ID, workspace, and terminal
  title. `TerminalSummary` in code.
- **Terminal row** — changes meaning: a sidebar row for one terminal, listed under its workspace
  after the sessions, showing the terminal icon and terminal title. The old single row is removed.
- **Terminal header** — the row above a terminal's xterm.js view with the terminal icon and terminal
  title. `terminal-header` in CSS.
- **Terminal menu** — the native context menu of a terminal row: Close Terminal.
  `showTerminalMenu()` in code.
- **Workspace menu** — now New Session, New Terminal, and Remove Workspace….
- **Selection** — the session or terminal chosen in the sidebar.
- Terminal title, terminal attachment, and detach keep their glossary meanings.

## Test plan

Terminal integration tests in `crates/ur/tests/terminal.rs`. `Daemon::start()` writes the state file
at `$XDG_STATE_HOME/ur/state.json` with one workspace, `home`, at the test directory, and `open()`
opens terminals in it.

- `terminal_starts_in_its_workspace`: `pwd -P` in a new terminal prints the canonicalized workspace
  path.
- `watch_shows_terminals_and_their_titles`: a watcher gets `terminal_changed` with title `sh` when
  the terminal opens, and again with `building` after `printf '\033]2;building\007'`. A new watch
  snapshot lists the terminal with `building`.
- `close_terminal_stops_its_programs`: with `sleep 1000` running in the foreground, `close_terminal`
  answers `Done`, a watching socket connection gets `terminal_exited`, the attached view's `pty`
  receiver closes, and attaching again is an error. The PTY closes only when `sleep` has exited too.
- `removing_a_workspace_stops_its_terminals`: after `remove_workspace`, the attached view's `pty`
  receiver closes.
- `a_detached_connection_gets_no_output`: one socket connection attaches and detaches, a second
  attaches and runs `echo after`. Once the second sees `after`, a request on the first connection
  answers with no `PTY` frame before it. Output fans out to every attached outbox at once, so a
  frame queued for the first would arrive before that response.
- `shell_exit_ends_the_terminal` also asserts that a watching socket connection gets
  `terminal_exited`.
- `attaching_an_unknown_terminal_is_an_error` becomes the table-driven
  `unknown_terminals_and_workspaces_are_errors`: attach, detach, and close an unknown terminal, and
  open a terminal in an unknown workspace.

Daemon tests in `crates/ur/src/daemon/tests.rs`: the test near line 966 opens its terminal in
`home`; `watch()` ignores `terminals`.

Webview unit tests in `app/src/store/watch.test.ts`:

- `terminals_follow_watch_events`: the snapshot's terminals, `terminal_changed` adding one and then
  replacing its terminal title, `terminal_exited` removing one, and `workspace_removed` removing the
  workspace's terminals.
- `workspace_terminals_keep_their_opening_order`: `workspaceTerminals()` returns one workspace's
  terminals in the order they were opened.

End-to-end tests in `app/e2e/terminal.test.ts`:

- `a terminal row shows the shell's name until a program sets a title`: `sh` in the terminal row and
  the terminal header, then the title an OSC 0 sequence sets.
- `a terminal starts in its workspace's directory`: a terminal opened in a second workspace is
  listed under it, and `pwd` prints its path.
- `Close Terminal removes the terminal's row`: through `Gui.request()`; the other terminal keeps
  working.
- `a terminal whose shell exits leaves the sidebar`: `exit`, then Select a session.
- `removing a workspace stops its terminals`.

Run `make e2e` at the end: this finishes a top-level item. The ad-hoc check is the todo item's
check: terminals in two workspaces, `npm run dev` surviving closing and reopening the GUI, switching
between terminals restoring each view, and Close Terminal stopping `npm run dev` (checked with
`ps`).

## Implementation plan

1. `crates/ur-client/src/protocol.rs`: change `OpenTerminal`, add `DetachTerminal`, `CloseTerminal`,
   `TerminalSummary`, `TerminalChanged`, and `WatchSnapshot.terminals`, and update the doc comments
   of `TerminalExited` and `WorkspaceRemoved`.
2. `crates/ur/src/daemon/state.rs`: add `terminals`, `add_terminal()`, `set_terminal_title()`,
   `remove_terminal()`, the snapshot's terminals, and the terminal removal in `remove_workspace()`.
   Update the `State` doc comment.
3. `crates/ur/src/daemon/terminal.rs`: `Terminals::new(state)`, `open(workspace)` with `cwd` and the
   first terminal title, `Title`, the reader's terminal title and exit reporting, `killer`,
   `close()`, and `detach()`. Update the doc comment on `open()`.
4. `crates/ur/src/daemon/ops.rs`, `server.rs`, and `mod.rs`: `remove_workspace()` closes the
   returned terminals; `handle()` dispatches the three terminal requests; `run()` builds
   `Terminals::new(state.clone())`.
5. `crates/ur/src/main.rs`: `workspace rm`'s help text says it stops the workspace's terminals.
6. `crates/ur/tests/terminal.rs` and `crates/ur/src/daemon/tests.rs`: the tests above. Run
   `cargo test` to regenerate the bindings in `app/src/ipc/bindings/`.
7. `app/src-tauri/src/gui_state.rs`, `link.rs`, `commands.rs`, and `main.rs`: the new `Selection`,
   `Saved` without `terminal`, `attachments`, `Link::detach()`, the event routing, the
   `attach_terminal` and `detach_terminal` commands, and their registration.
8. `app/src/ipc/index.ts`: `WatchEvent` gains `terminal_changed` and `terminal_exited`;
   `attachTerminal(terminal, rows, cols, onOutput)` and `detachTerminal(terminal)`.
9. `app/src/store/watch.ts` and `watch.test.ts`: `terminals` in `WatchState`, the reducer cases, a
   disconnect clearing terminals, and `workspaceTerminals()`.
10. `app/src/actions.ts`: `newTerminal()`, which selects the new terminal, `closeTerminal()`,
    `showTerminalMenu()`, New Terminal in `showWorkspaceMenu()`, and "Its terminals will be closed."
    in `removeWorkspace()` when the workspace has terminals.
11. `app/src/components/Sidebar.tsx`, `TerminalPane.tsx`, `app/src/App.tsx`, and
    `app/src/styles.css`: the terminal rows and terminal menu, the terminal header, per-terminal
    attach and detach, the selection check, and the removal of the global Terminal row and its
    `margin-top`.
12. `app/e2e/harness.ts`: the `home` workspace, and `Gui.showTerminal()` opening and selecting a
    terminal. `session.test.ts`: the empty states and returning session tests. `terminal.test.ts`:
    the tests in the Test plan.

## Documentation updates

- `docs/agents/architecture.md`: the opening paragraph says terminals last until closed, their
  workspace is removed, or the daemon exits. Wire protocol: `terminal_exited` goes to watching and
  attached socket connections, and the watch events list gains `terminal_changed`. Terminals in the
  daemon: the workspace path as working directory, the terminal title from `set_window_title()`,
  Close Terminal's `SIGHUP` and its limitation, and `detach_terminal`. GUI state: drop the GUI's one
  terminal. Project layout: `commands.rs` gains `detach_terminal`.
- `docs/agents/glossary.md`: the Naming entries above, and the watch definition names terminal
  summaries.
- `docs/agents/testing.md`: `Gui.showTerminal()` opens a terminal in the test's `home` workspace and
  selects its terminal row; `TestEnvironment` adds that workspace; the terminal guarantees.
- `AGENTS.md`: the entries for `protocol.rs`, `terminal.rs`, `state.rs`, `ops.rs`, `link.rs`,
  `commands.rs`, `gui_state.rs`, `app/src/ipc/`, `watch.ts`, `actions.ts`, `Sidebar.tsx`,
  `TerminalPane.tsx`, `App.tsx`, and `harness.ts`.
