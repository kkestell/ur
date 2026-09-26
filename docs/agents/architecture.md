# ur architecture

ur is an ACP client. Ox (`~/projects/ox`) is the server we develop and test against, not part of
ur's architecture. Switching to another server with compatible ACP capabilities should require
changing its launch configuration, not ur's code. When all milestones in `todo.md` are done, you can
run ten agent sessions across three repositories, close the window, come back later, see which
sessions need attention, and approve, answer, or cancel them from the GUI or the command line.
Terminal panes keep shells and TUI apps running when the GUI closes. Reopening a pane restores the
running application's screen. Terminals last until closed, their workspace is removed, or the daemon
exits.

This is a hobby project. Keep the implementation small, make the ordinary workflow work, and use
testing to find the next problems worth solving. Prefer explicit limitations to speculative recovery
machinery or abstractions for cases we have not encountered.

## ACP boundary

The [ACP spec](https://agentclientprotocol.com/protocol/v1/initialization) is the contract. Use the
SDK's protocol types and capability negotiation. Server names, tool names, tool argument schemas,
model names, modes, permission labels, and session or tool IDs have no special meaning to ur.

- One configured server per daemon. Its executable and argument list live in the `[server]` table of
  `$XDG_CONFIG_HOME/ur/config.toml` (under `~/.config` when unset). The one-shot client and daemon
  use this same configuration. Development uses:

  ```toml
  [server]
  command = "ox"
  args = []
  ```

- Call optional methods only when supported. List and load enable saved history; without them, ur
  still supports sessions created during the current ACP connection. Delete is available only when
  advertised. Image attachments require image-prompt support. Config selectors, slash commands, and
  usage indicators come from the data the server supplies.
- Advertise only client capabilities ur implements. Initially, client `fs/*` and ACP `terminal/*`
  services are unadvertised; ur's user terminal panes are separate. A server that requires those
  services needs that standard ACP support added, not a server-specific adapter.
- Use the server's normal authentication setup for development. Surface authentication requirements
  and errors from ACP without hard-coded login commands or credential handling for Ox.
- Wireframe model names, modes, permission choices, commands, and workspace names are examples. The
  UI uses server-provided labels or neutral text. Ox's database, tools, subagents, limits, and
  notification timing are not client requirements.

Keep this as ordinary configuration and protocol handling. Ox remains the end-to-end test server;
SDK-based test agents exercise the protocol without depending on Ox's implementation.

## Decisions

### One ACP server process per daemon

The daemon starts the configured server as a child and keeps one ACP connection to it. Sessions
across workspaces use that connection. If the server exits, active turns fail; the daemon starts it
again and restores history where supported. The supervisor waits 1 second before starting the server
again. Each failed start doubles the wait, up to 30 seconds, and a successful `initialize` resets
it.

### The ACP server owns saved history

The daemon saves no transcripts. It keeps each loaded session's transcript in memory as a list of
entries of three kinds: an ACP `SessionUpdate` it received, a user prompt it sent, and a turn error.
After a restart, the daemon rebuilds the list from `session/load` when supported, using its replayed
transcript. Live updates and replay use the same entry handling. A load builds a fresh list and
replaces the previous transcript on success; it never appends replay to the old transcript. Rejected
prompts and turn errors exist only in memory and are gone after a restart.

For each workspace, the sidebar combines newly created sessions with those returned by
`session/list` when supported, following its pagination and using their titles and last activity.
The daemon calls `session/list` for every workspace after each `initialize`, and for a workspace
when it is added. A workspace whose list fails shows only the sessions the daemon already has.
`session_info_update`, when sent, live or replayed, keeps those details current. A session with no
title yet shows as "New session". Selecting a session shows its thread; the daemon loads its saved
transcript automatically the first time it is needed for viewing or prompting, when loading is
supported. Newly created sessions are already loaded.

A load starts when a daemon client subscribes to an unloaded session that is not `Failed`, when a
daemon client prompts an unloaded session, and, after the server restarts, for every unloaded
session with subscribers. A `subscribe` that starts a load, or arrives during one, gets its session
snapshot and its response when the load ends. A prompt that starts a load answers once
`session/load` is sent, sets `Working`, and sends the prompt when the load succeeds. If the
transcript being replaced ends with a turn error entry, that entry is kept after the replay, so the
error of a turn the server exit interrupted survives the reload. Without `loadSession`, sessions
from an earlier ACP connection keep their transcripts in memory but cannot be prompted.

The daemon's own state file (`$XDG_STATE_HOME/ur/state.json`, under `~/.local/state` when the
variable is unset) holds workspace names and absolute paths. The daemon reads it at startup and
stops with an error naming the file when it cannot. Saved session discovery comes from the server.
The GUI stores which threads are visible as part of its layout.

A workspace path is made absolute when the workspace is added and stored as is. Every `session/new`,
`session/load`, and `session/list` call for that workspace passes that exact string.

### Session status

The daemon owns one status for each session. Every client reads the same value. One operation guard
per session covers prompt, load, and delete. The daemon takes it before calling the server and
rejects a conflicting request as busy without changing the transcript or status. Delete cancels an
active prompt and keeps the session reserved until deletion finishes.

| Status                         | Enters when                                                                                                                | Leaves when                                                                   |
| ------------------------------ | -------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| `Idle { last_stop }`           | the prompt returns a stop reason, or a session is discovered in the server's list                                          | a prompt is sent                                                              |
| `Working`                      | a prompt is sent, the last pending permission request is answered, or cancellation starts                                  | the prompt returns, or a permission request arrives before cancellation       |
| `NeedsPermission { requests }` | a permission request arrives                                                                                               | the last pending request is answered, cancellation starts, or the prompt ends |
| `Failed { message }`           | the prompt returns a JSON-RPC error, a load fails, or the server exits while the session is `Working` or `NeedsPermission` | a prompt is sent                                                              |

`requests` holds every pending permission request for the session, oldest first. ACP permits several
requests to be pending; their JSON-RPC IDs are opaque, so the daemon gives each a request ID of its
own, as described under Permissions. A prompt sent to a session whose load failed loads it first; if
the load fails again, the session returns to `Failed`.

Each session also has an `unread` flag. It turns on when a turn ends or fails while no client has
the session focused, and turns off when a client focuses it or sends it a prompt. A session **needs
attention** when it is `NeedsPermission`, `Failed`, or unread. The sidebar sorts workspaces and
sessions by this.

A client focuses the set of sessions it is showing. The GUI focuses every session shown as the
active tab of a pane while its window has focus, and focuses nothing when the window loses focus.
`ur read` focuses the session it reads, so reading a session clears its unread flag, and a session
followed with `--follow` does not become unread. A client's focus clears when it disconnects.

### Permissions

The daemon holds the ACP responder for each pending permission request and sends the request to
every client. It numbers pending permission requests from 1, across all sessions, and clients answer
by that request ID. The JSON-RPC ID can be a string, a number, or null, so the daemon does not use
it. Any client can answer, and the first answer wins. The daemon then tells every client the request
is resolved. When a client cancels a session, the daemon sends `session/cancel` and answers every
pending permission request for that session with `Cancelled`, which the ACP spec requires. Until the
prompt returns, the session stays busy and shows `Working`; any further permission request is
answered `Cancelled`. When the prompt ends or the server disconnects, the daemon clears any
remaining pending requests and tells clients they are resolved.

The GUI draws one button per option, using the label and kind in the request. The CLI's `approve`
and `deny` answer the session's oldest pending request with its first `allow_*` or `reject_*`
option. The GUI's shortcuts do the same by option kind: Command+Y for `allow_once`, Command+Shift+Y
for `allow_always`, Command+Option+Z for `reject_once`, and Command+Shift+Option+Z for
`reject_always` on macOS. Other platforms use Ctrl in place of Command and Alt in place of Option.

### Sessions, tabs, and workspaces

Sessions appear automatically under their workspace. New Session creates one; Delete, when
supported, removes it from the server after confirmation. If a turn is running, Delete cancels it,
waits for the prompt to return, then calls `session/delete`. Choosing a session or terminal in the
sidebar activates its tab if it has one, wherever it is, and otherwise opens it in the active pane.
Closing a tab only changes the layout.

Command+N opens a new session and Command+Shift+N a new terminal on macOS. Other platforms use Ctrl
in place of Command. Each opens in the workspace of the active tab, or in the first workspace in the
sidebar when no tab is open. Command+W on macOS or Ctrl+W on other platforms closes the active tab.
Command+Shift+T or Ctrl+Shift+T reopens the most recently closed tab in the active pane. The GUI
keeps closed tabs only in memory for its current run; it skips tabs whose session or terminal is
gone or whose tab was opened again from the sidebar.

Remove Workspace cancels its running turns and stops its terminals after confirmation, then removes
the workspace from the sidebar. Saved history and the lifecycle of agent tools and background jobs
remain the server's concern.

### User messages

When the daemon sends `session/prompt`, it adds a user prompt entry to the session's in-memory
transcript. If the server rejects the prompt with an error before the turn starts (for example, the
input is too large or the model does not accept images), the turn error described below appears just
after the prompt. When `session/load` is supported, user messages come back from its replay.

### Turn errors

When `session/prompt` returns a JSON-RPC error, or the server exits during a turn, the daemon
appends a turn error entry with the message and sets the session `Failed`. The entry stays in the
thread; the next prompt clears the `Failed` status. This is the single error-entry rule for both
rejected prompts and failures during a turn; there is no second error stored on the user prompt.

### Config options

The daemon passes through `configOptions` from session responses and `config_option_update`
notifications. The GUI builds selectors from their labels, values, and current selections; no model,
effort, or mode is hard-coded. If the server supplies no config options, those selectors are absent.

The controls follow the standard
[session config options](https://agentclientprotocol.com/protocol/v1/session-config-options) schema.

### Wire protocol

The socket uses one frame format: a one-byte tag, a big-endian `u32` length, and the payload. Tags:

- `JSON`, both directions: a serde-encoded message. From the client, a request with an ID. From the
  daemon, a response with the request's ID, or an event.
- `PTY`, both directions: a terminal ID followed by raw bytes. From the client, input. From the
  daemon, output. This keeps terminal output out of JSON.

Events contain ACP schema types (`SessionUpdate`, `RequestPermissionRequest`, `SessionConfigOption`)
unchanged. The GUI groups them for display as described below. JSON client requests:

```text
watch
add_workspace | remove_workspace
new_session(workspace) | delete_session(session)
subscribe(session) | focus(sessions)
prompt(session, content) | cancel(session)
answer_permission(session, request_id, option_id)
set_config_option(session, config_id, value)
open_terminal(workspace) | attach_terminal(terminal, rows, cols) | detach_terminal
terminal_resize | close_terminal
```

`terminal_exited` tells every watching or attached daemon client that a terminal's shell exited and
the daemon removed the terminal. Each socket connection gets it once, after all of that terminal's
output.

There are three levels of events. `watch` covers the sidebar: workspaces, their sessions and
terminals, each session's title, status, and unread flag, each terminal's title, and the server's
capabilities. Pending permission requests travel in the session status, so every watching client
receives them. After the watch snapshot, the daemon sends `capabilities_changed` after each
successful `initialize`, `workspace_added`, `workspace_removed` (which also removes the workspace's
sessions and terminals), `session_changed` with the session's whole summary when a session is added
or its status or unread flag changes, `session_deleted` when a session is deleted,
`terminal_changed` with the terminal's whole summary when a terminal is opened or its terminal title
changes, and `terminal_exited`. `subscribe` covers one session's content: its transcript and config
options. After the session snapshot, the daemon sends each transcript entry and
`config_options_changed` when the config options change outside a load. A subscribed session removed
with its workspace, or deleted, gets `session_removed`. `attach_terminal` covers one terminal's
initial screen restoration and live output, using the terminal attachment format established in
milestone 0. For `watch` and `subscribe`, under one lock the daemon queues a snapshot and registers
the client for live events. Socket writes happen outside the lock. Each connection gets the snapshot
followed by live changes in order. A client that cannot keep up is dropped. Transcript replacement
after a load sends a fresh session snapshot, which clients use to replace their local state.

### Project layout

One Cargo workspace:

- `crates/ur`: the `ur` binary, which is the daemon and the CLI.
- `crates/ur-client`: the frame format, the request and event types, and an async client for the
  socket. The CLI and the GUI both use it.
- `crates/ur-fake-server`: the fake server, a scripted server built with the SDK's
  `Agent.builder()`. The daemon's tests run its library in process, and the end-to-end suite's
  daemon launches its binary from the config file.
- `app/`: the Tauri 2 app. `app/src` is the frontend, React and TypeScript built with Vite.
  `app/src-tauri` is the app's Rust core, a workspace member that depends on `ur-client`.
  `pnpm tauri dev` runs it against a daemon that is already running.

```text
crates/ur/src/
  main.rs         clap: daemon | agent-run | new | prompt | read | ls | wait |
                  approve | deny | cancel | workspace
  config.rs       config.toml: server command and args, on_event
  one_shot.rs     agent-run; speaks ACP directly, no daemon
  cli/            one file per subcommand; each uses ur_client::Client only
  daemon/
    mod.rs        start(): read state.json and the config file, start the
                  supervisor, wait for initialize, then serve the listener
    state.rs      State: workspaces, sessions, terminals, subscriber lists.
                  Pure: no IO, no await.
    acp.rs        supervisor loop around connect_with, restart with backoff,
                  Agent facade over ConnectionTo<Agent>
    ops.rs        prompt, load, delete, cancel, answer_permission, with
                  responses handled in on_receiving_result callbacks
    terminal.rs   portable-pty plus vt100, one struct per terminal
    server.rs     Unix socket accept loop, per-connection reader and writer,
                  Outbox
    hooks.rs      notifications and on_event (milestone 11)

crates/ur-client/src/
  frame.rs        Frame::Json(Bytes) | Frame::Pty { id, Bytes };
                  tokio_util Encoder and Decoder
  protocol.rs     Request, Response, Event, Entry, Status, WatchSnapshot,
                  SessionSnapshot
  client.rs       Client: a Clone handle. connect(path),
                  request(Request) -> Response, events() -> Receiver<Event>,
                  pty(id) -> Receiver<Bytes>, pty_input(id, bytes)

app/src-tauri/src/
  main.rs         builder, dialog plugin, opener plugin, managed Link, window
                  focus hook
  link.rs         owns ur_client::Client; reconnect loop; Desired { watch,
                  subscribed, attached, focus }; forwards events to app.emit
                  and PTY bytes to the terminal Channel
  commands.rs     request(req: Request) -> Response, attach_terminal,
                  detach_terminal, terminal_input, set_visible, layout,
                  save_layout
  terminal.rs     terminal ID to Channel<tauri::ipc::Response>
  gui_state.rs    gui.json

app/src/
  ipc/            request(), onWatch(), onSession(id), attachTerminal(id),
                  typed by the generated bindings
  store/          watch.ts, sessions.ts as Map<id, ThreadState> with
                  useSession(id), terminals.ts
  transcript/     reduce.ts: (ThreadState, Event) -> ThreadState, pure and
                  unit tested; blocks.ts display types
  components/     Sidebar, Thread, Editor, ConfigPicker, UsageIndicator,
                  Permission, TerminalPane, Layout, Tab, StatusMark
  hooks/          useTerminal(id)
  layout.ts       TabItem, openTab(), goneTabs(), visibleSessions()
  actions.ts      folder picker, confirmations, native menus
  slash.ts        command list matching
  usage.ts        usage indicator text
  keys.ts
```

`ur-client`'s `Client` is a cloneable handle over one writer task and a pending map keyed by request
ID. Every CLI subcommand and the Tauri core use it unchanged. TypeScript bindings for `protocol.rs`
come from `ts-rs`, with ACP payload fields overridden to the types exported by
`@agentclientprotocol/sdk`, so the webview writes no protocol types by hand.

### Daemon architecture

- One `Arc<Mutex<State>>` with a std mutex, never held across an await. Every mutation is a method
  on `State` that queues its events on the affected outboxes while the lock is held. An outbox is a
  bounded `mpsc::Sender<Frame>` used with `try_send`, which never waits, so `State` still does no IO
  and no awaiting; a full outbox closes that connection. A subscriber's snapshot and every later
  entry are queued under the same lock, in transcript order, which gives the ordering guarantee
  under Wire protocol.
- ACP handlers apply what they receive to `State` directly. The notification handler locks `State`,
  appends the ACP update entry, queues its event, and returns without awaiting. The SDK's dispatch
  loop waits for each handler, so it is held only for the lock, and updates reach `State` in the
  order the server sent them. Replay from `session/load` arrives through the same handler and the
  same `apply_update`.
- The supervisor calls `connect_with`, sends `initialize`, lists every workspace's saved sessions,
  stores a clone of `ConnectionTo<Agent>` and the server's capabilities in `State`, reloads
  subscribed sessions, and awaits the ACP connection's close. The daemon accepts socket connections
  only after the first `initialize`, so no request sees a server that is still starting. A server
  that has not answered `initialize` and the lists after 30 seconds counts as a failed `initialize`,
  so it cannot block terminals. Each successful `initialize` starts a new generation, carried on
  every prompt and load result; `State` ignores anything from an earlier generation. On exit the
  supervisor fails `Working` and `NeedsPermission` sessions, clears pending requests, marks every
  session unloaded, releases every operation guard, and starts the server again with backoff.
- The guard lives in `State` as `session.op`, an `Op`: `Prompt` while a turn runs, `Load` while
  `session/load` is in flight, or `Delete` while `session/delete` is in flight. `Prompt` can hold
  the waiting delete: a `delete_session` that cancelled the turn. The `session/prompt` callback
  sends its `session/delete` under the same lock, so no other request takes the guard in between.
  `Load` holds the replayed entries, which become the transcript only on success, and the
  `subscribe` requests waiting for the load. During a load, `apply_update` adds to the replay and
  sends nothing to subscribers. Under the lock: if `op` is set, return busy; otherwise send the
  request through the connection clone, set `op`, and, for a prompt, append the user prompt entry
  and set `Working`. Sending under the lock means a failed send leaves nothing to undo, and the
  server's first update waits for the lock, so it follows the user prompt entry. The op handles the
  response in an `on_receiving_result` callback, never `block_task()`: the SDK runs that callback
  before it dispatches the server's next message. So the `session/new` callback adds the session
  before the server's first update for it is handled, and the `session/prompt` callback clears `op`
  only after the turn's last update is in the transcript. The callbacks always return `Ok`, because
  an error from one shuts down the ACP connection. A prompt or load callback ignores the error a
  request gets when the ACP connection closes, which can arrive before or after the supervisor sees
  the close; the supervisor records that server exit, so each interrupted turn gets one turn error
  entry. Delete holds `op` through cancel, wait, and delete.
- Pending permission requests hold their SDK `Responder` in `Session.responders`, keyed by request
  ID; `answer_permission` and cancellation respond through it.
- `Entry`, `Status`, and the snapshot structs are defined in `ur-client` and used as-is inside
  `State`. There is no conversion layer.

### GUI architecture

The GUI is a single-window Tauri app. Its Rust core is the socket client: it connects to the daemon,
retries until the daemon is up, and is the only part of the app that speaks the wire protocol. The
webview never touches the socket.

- One Tauri command, `request`, takes a wire-protocol `Request` and returns the daemon's `Response`.
  The tagged `Request` enum carries the name and arguments, so adding a request touches
  `protocol.rs` and the daemon only. The other commands are `attach_terminal`, `detach_terminal`,
  `terminal_input`, `layout`, `save_layout`, `connection`, and `set_visible`. `attach_terminal` is
  separate because it takes the terminal's `Channel`, which `request` cannot carry. `connection`
  gives the webview the current connection when it starts, since a `connection` event emitted before
  its listener is installed is lost.
- Daemon events reach the webview as Tauri events: `watch` events under one name, and every
  subscribed session's events under the one name `session`, with the webview dispatching on the
  payload's session ID. Tauri event names allow only alphanumerics, `-`, `/`, `:`, and `_`, and
  session IDs are opaque, so a name that carries the session ID is not safe. The webview keeps the
  sidebar and each subscribed session's state in stores outside React, keyed by session ID, so
  closing a tab keeps the transcript and reopening does not refetch. Components read the stores
  through `useSyncExternalStore` or zustand.
- One transcript reducer groups consecutive message and thought chunks, combines a user's text and
  image parts, and updates tool calls by ID. It keeps each tool call's content, which a
  `tool_call_update` with content replaces whole. A fresh snapshot resets that state and uses the
  same reducer as live events. The reducer is the only code that knows ACP update shapes; components
  render the blocks it derives. Whether a Thinking row or tool call is expanded is component state,
  not part of the blocks.
- Terminal output goes through a `Channel<tauri::ipc::Response>` of raw bytes, one per attached
  terminal, so it is never JSON-encoded. Terminal input goes to the core through a `terminal_input`
  command, which sends a binary `PTY` frame.
- The webview tells the core which sessions are visible in panes through `set_visible`. The core
  combines that with the window's focus, which it gets from `WindowEvent::Focused`, and sends
  `focus` to the daemon as described under Session status, on each `set_visible` and each window
  focus change. Focus is part of the desired set, replayed after the subscriptions.
- Pending permission requests render from the watch store, where they arrive in the session status,
  not from the thread state. `transcript/permissions.ts` places them among the blocks: each takes
  the place of the tool call block with its tool call ID, or follows the last block.
- Native pieces come from Tauri and are used from the webview: the dialog plugin for the folder
  picker, confirmations, and error messages, the opener plugin, which opens links in agent messages
  in the default browser, and the menu API's `Menu.popup()` for the workspace, session, terminal,
  and new menus. Image files are dropped on the editor as HTML drop events, since the window's
  `dragDropEnabled` is off, and the webview reads them into image content. Keyboard shortcuts are
  handled in the webview.
- The core writes the GUI state file described below.

The `useSession` hook sends `subscribe` the first time a session is used and never unsubscribes: the
wire protocol has no `unsubscribe`, and the session store keeps every subscribed session's thread so
reselecting it does not refetch. A session removed with its workspace leaves both the store and the
desired set, so it is subscribed afresh if it comes back. The Link keeps the desired set: watch,
subscribed sessions, terminal attachments with their sizes, and focus. After a disconnect it
reconnects and replays that set. The webview has no reconnect logic: snapshots replace store state,
terminal views restore through the terminal attachment path, and a `connection` event drives the
no-connection empty state. Requests interrupted by disconnection report an error and are not
automatically resent; the user can inspect the session before trying again.

### Terminals in the daemon

The daemon owns each login shell through `portable-pty` and retains the terminal state while the GUI
is closed. A terminal belongs to a workspace, and its shell starts in the workspace path. Its
terminal title is the shell's file name until a program sets one with OSC 0 or OSC 2, which the
parser reports through `vt100::Callbacks::set_window_title()`. `State` holds each terminal's summary
for watch, and `Terminals` holds the PTYs, parsers, and attachments under their own mutexes and
reports opens, terminal title changes, and exits to `State`. Locks are taken in the order `State`,
the terminal map, then a terminal's output. A terminal pane is an xterm.js `Terminal` with the fit
addon. On attachment, the GUI restores the current screen and resumes live output. `onData` goes
through `terminal_input` to the PTY, and `onResize` goes to `terminal_resize`.

Reliable restoration of running TUI apps is a requirement. Milestone 0 must start with the Rust
`vt100` crate: feed PTY output into `vt100::Parser` while the GUI is open or closed, then use
`Screen::state_formatted()` as the starting point for the snapshot sent to xterm.js on attachment,
followed by live output. Milestone 0 validates this integration with real TUI apps and establishes
the terminal attachment format used by the rest of the application.

Each terminal holds its `vt100::Parser` and its list of attached outboxes under one std mutex. The
reader thread locks it, feeds the parser, and fans the bytes out to every attached outbox. Attach
locks the same mutex, takes `state_formatted()` and the size as the snapshot, and adds its outbox.
No output can arrive between the snapshot and the registration.

The terminal attachment format, established in milestone 0:

- `attach_terminal(terminal, rows, cols)` first resizes the PTY and the parser if the size differs,
  so the screen snapshot matches the daemon client's view. The application redraws at the new size
  through live output.
- The screen snapshot is `state_formatted()`, prefixed with `ESC [ ? 1049 h` when the alternate
  screen is active. `state_formatted()` does not switch to the alternate screen, and without the
  prefix a restored full-screen application would leave its last frame in the normal buffer when it
  quits.
- The screen snapshot is the attachment's first `PTY` frame, queued before the outbox is registered
  and before the response, so a daemon client registers its `pty` receiver before sending
  `attach_terminal`.
- The parser keeps no scrollback. The normal buffer's contents and scrollback from before the
  attachment are not restored.

`detach_terminal`, or losing the GUI, ends the terminal attachment and leaves the shell and its
applications running. Close Terminal and Remove Workspace stop the corresponding terminals: the
daemon sends `SIGHUP` to the shell, which passes it to its jobs, and the kernel sends it to the
foreground process group when the shell exits. The terminal ends when its PTY closes, the same way
as a shell that ran `exit`. A program that ignores `SIGHUP` and keeps the PTY open keeps its
terminal listed.

### GUI state

The core keeps the GUI's layout in `$XDG_STATE_HOME/ur/gui.json`, keyed by socket path, as
dockview's serialized layout, which includes each pane's active tab and the active pane. The webview
saves it on every layout change. Tabs refer to sessions and terminals by ID. When the GUI restores
the layout, it drops tabs whose session or terminal no longer exists.

### Transport

The daemon listens on a Unix socket, `$UR_SOCKET` or `$TMPDIR/ur.sock`. There is no TCP listener and
no auth code.

### Runtime

The daemon runs on Tokio, because the ACP Rust SDK (`agent-client-protocol` 2.1.0) is async. The
supervisor in `daemon/acp.rs` uses `Client.builder()` with the handlers for `SessionNotification`
and `RequestPermissionRequest` described under Daemon architecture, and `connect_with` to an
`AcpAgent` built from the configured command and arguments (see the SDK's
`examples/yolo_one_shot_client.rs`). Requests to the server go through a clone of
`ConnectionTo<Agent>`, and ops handle their responses in `on_receiving_result` callbacks as
described under Daemon architecture. Only `initialize` and `session/list` are awaited with
`block_task()`: the supervisor, which is not a handler, awaits `initialize` and the lists after it,
and a spawned task awaits the list for a workspace that was just added. PTY reads stay on a blocking
thread. The app's core runs the socket connection on Tauri's Tokio runtime.
