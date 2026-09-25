# ur plan

ur is an ACP client. Ox (`~/src/ox`) is the server we develop and test
against, not part of ur's architecture. Switching to another server with
compatible ACP capabilities should require changing its launch configuration,
not ur's code. When all milestones are done, you can run ten agent sessions
across three repositories, close the window, come back later, see which
sessions need attention, and approve, answer, or cancel them from the GUI or
the command line.
Terminal panes keep shells and TUI apps running when the GUI closes.
Reopening a pane restores the running application's screen. Terminal sessions
last for the lifetime of the daemon.

This is a hobby project. Keep the implementation small, make the ordinary
workflow work, and use testing to find the next problems worth solving.
Prefer explicit limitations to speculative recovery machinery or abstractions
for cases we have not encountered.

## ACP boundary

The [ACP protocol](https://agentclientprotocol.com/protocol/v1/initialization)
is the contract. Use the SDK's protocol types and capability negotiation.
Server names, tool names, tool argument schemas, model names, modes, permission
labels, and session or tool IDs have no special meaning to ur.

- One configured server per daemon. Its executable and argument list live in
  `$XDG_CONFIG_HOME/ur/config.toml` (under `~/.config` when unset).
  Development uses `command = "ox"` and `args = []`. The one-shot client and
  daemon use this same configuration.
- Call optional methods only when supported. List and load enable saved
  history; without them, ur still supports sessions created during the current
  connection. Delete is available only when advertised. Image attachments
  require image-prompt support. Config selectors, slash commands, and usage
  indicators come from the data the server supplies.
- Advertise only client capabilities ur implements. Initially, client
  `fs/*` and ACP `terminal/*` services are unadvertised; ur's user terminal
  panes are separate. A server that requires those services needs that
  standard ACP support added, not a server-specific adapter.
- Use the server's normal authentication setup for development. Surface
  authentication requirements and errors from ACP without hard-coded login
  commands or credential handling for Ox.
- Wireframe model names, modes, permission choices, commands, and workspace
  names are examples. The UI uses server-provided labels or neutral text.
  Ox's database, tools, subagents, limits, and notification timing are not
  client requirements.

Keep this as ordinary configuration and protocol handling. Ox remains the
end-to-end test server; SDK-based test agents exercise the protocol without
depending on Ox's implementation.

## Decisions

### One ACP server process per daemon

The daemon starts the configured server as a child and keeps one ACP
connection to it. Sessions across workspaces use that connection. If the
server exits, active turns fail; the daemon starts it again and restores
history where supported (milestone 4).

### The ACP server owns saved history

The daemon saves no transcripts. It keeps each loaded session's transcript in
memory as a list of entries of three kinds: an ACP `SessionUpdate` it
received, a user prompt it sent, and a turn error. After a restart, the daemon
rebuilds the list from `session/load` when supported, using its replayed
transcript. Live updates and replay use the same entry handling. A load builds a fresh list
and replaces the previous transcript on success; it never appends replay to
the old history. Rejected prompts and turn errors exist only in memory and
are gone after a restart.

For each workspace, the sidebar combines newly created sessions with those
returned by `session/list` when supported, following its pagination and using
their titles and last activity. `session_info_update`, when sent, keeps those
details current during a turn. A session with no title yet shows
as "New session". Selecting a session shows its thread; the daemon loads its
saved transcript automatically the first time it is needed for viewing or
prompting, when loading is supported. Newly created sessions are already loaded.

The daemon's own state file (`$XDG_STATE_HOME/ur/state.json`, under
`~/.local/state` when the variable is unset) holds workspace names and absolute
paths. Saved session discovery comes from the server. The GUI stores which
threads are visible as part of its layout.

A workspace path is made absolute when the workspace is added and stored as
is. Every `session/new`, `session/load`, and `session/list` call for that
workspace passes that exact string.

### Session status

The daemon owns one status for each session. Every client reads the same value.
One operation guard per session covers prompt, load, and delete. The
daemon takes it before calling the server and rejects a conflicting request as busy
without changing the transcript or status. Delete cancels an active prompt
and keeps the session reserved until deletion finishes.

| Status | Enters when | Leaves when |
|---|---|---|
| `Idle { last_stop }` | the prompt returns a stop reason, or a session is discovered in the server's list | a prompt is sent |
| `Working` | a prompt is sent, the last pending permission request is answered, or cancellation starts | the prompt returns, or a permission request arrives before cancellation |
| `NeedsPermission { requests }` | a permission request arrives | the last pending request is answered, cancellation starts, or the prompt ends |
| `Failed { message }` | the prompt returns a JSON-RPC error, a load fails, or the server exits while the session is `Working` or `NeedsPermission` | a prompt is sent |

`requests` holds every pending permission request for the session, oldest
first. ACP permits several requests to be pending; their IDs are opaque. A
prompt sent to a session whose load failed loads it first; if the load fails
again, the session returns to `Failed`.

Each session also has an `unread` flag. It turns on when a turn ends or fails
while no client has the session focused, and turns off when a client focuses
it. A session **needs attention** when it is `NeedsPermission`, `Failed`, or
unread. The sidebar sorts workspaces and sessions by this.

A client focuses the set of sessions it is showing. The GUI focuses every
session visible in a pane while its window has focus, and focuses nothing when
the window loses focus. A client's focus clears when it disconnects.

### Permissions

The daemon holds the ACP responder for each pending permission request and
sends the request to every attached client. Any client can answer, and the
first answer wins. The daemon then tells every client the request is resolved.
When a client cancels a session, the daemon sends `session/cancel` and
answers every pending permission request for that session with `Cancelled`,
which the ACP spec requires. Until the prompt returns, the session stays busy
and shows `Working`; any further permission request is answered `Cancelled`.
When the prompt ends or the server disconnects, the daemon clears any remaining
pending requests and tells clients they are resolved.

The GUI draws one button per option, using the label and kind in the request.
The CLI's `approve` and `deny` answer the session's oldest pending request with its first `allow_*` or `reject_*` option.

### Sessions, tabs, and workspaces

Sessions appear automatically under their workspace. New Session creates one;
Delete, when supported, removes it from the server after confirmation. If a
turn is running, Delete cancels it, waits for the prompt to return, then calls
`session/delete`.
Selecting a session displays it, and closing its tab only changes the layout.

Remove Workspace cancels its running turns and stops its terminals after
confirmation, then removes the workspace from the sidebar. Saved history and
the lifecycle of agent tools and background jobs remain the server's concern.

### User messages

When the daemon sends `session/prompt`, it adds a user prompt entry to the
session's in-memory transcript. If the server rejects the prompt with an error
before the turn starts (for example, the input is too large or the model does not
accept images), the turn error described below appears just after the prompt.
When history loading is supported, user messages come back from its replay.

### Turn errors

When `session/prompt` returns a JSON-RPC error, or the server exits during a
turn, the daemon appends a turn error entry with the message and sets the session
`Failed`. The entry stays in the thread; the next prompt clears the `Failed`
status. This is the single error-entry rule for both rejected prompts and
failures during a turn; there is no second error stored on the user prompt.

### Config options

The daemon passes through `configOptions` from session responses and
`config_option_update` notifications. The GUI builds selectors from their
labels, values, and current selections; no model, effort, or mode is hard-coded.
If the server supplies no config options, those selectors are absent.

The controls follow the standard
[session config options](https://agentclientprotocol.com/protocol/v1/session-config-options)
schema.

### Wire protocol

The socket uses one frame format: a one-byte tag, a big-endian `u32` length,
and the payload. Tags:

- `JSON`, both directions: a serde-encoded message. From the client, a request
  with an ID. From the daemon, a response with the request's ID, or an event.
- `PTY`, both directions: a terminal ID followed by raw bytes. From the client,
  input. From the daemon, output. This keeps terminal output out of JSON.

Events contain ACP schema types (`SessionUpdate`, `RequestPermissionRequest`,
`SessionConfigOption`) unchanged. The GUI groups them for display as described
below. JSON client requests:

```text
watch
add_workspace | remove_workspace
new_session | delete_session
subscribe(session) | unsubscribe(session) | focus(sessions)
prompt(session, content) | cancel(session)
answer_permission(session, request_id, option_id)
set_config_option(session, option_id, value)
open_terminal(workspace) | attach_terminal(terminal, rows, cols) | detach_terminal
terminal_resize | close_terminal
```

There are three levels of events. `watch` covers the sidebar: workspaces,
their sessions and terminals, each session's title, status, and unread
flag, and each terminal's title. `subscribe` covers one session's content: its
transcript, config options, and pending permission requests. `attach_terminal`
covers one terminal's initial screen restoration and live output, using the
restoration format established in milestone 0. For `watch` and `subscribe`,
under one lock the daemon queues a snapshot and registers the client for live
events. Socket writes happen
outside the lock. Each connection gets the snapshot followed by live changes
in order. A client that cannot keep up is dropped. Transcript replacement
after a load sends a fresh session snapshot, which clients use to replace
their local state.

### Project layout

One Cargo workspace:

- `crates/ur`: the `ur` binary, which is the daemon and the CLI.
- `crates/ur-client`: the frame format, the request and event types, and an
  async client for the socket. The CLI and the GUI both use it.
- `app/`: the Tauri 2 app. `app/src` is the frontend, React and TypeScript
  built with Vite. `app/src-tauri` is the app's Rust core, a workspace member
  that depends on `ur-client`. `pnpm tauri dev` runs it against a daemon that
  is already running.

```text
crates/ur/src/
  main.rs         clap: daemon | agent-run | new | prompt | read | ls | wait |
                  approve | deny | cancel | workspace
  config.rs       config.toml: server command and args, on_event
  one_shot.rs     agent-run; speaks ACP directly, no daemon
  cli/            one file per subcommand; each uses ur_client::Client only
  daemon/
    mod.rs        start(): read state.json, spawn the supervisor, the ingest
                  loop, and the listener
    state.rs      State: workspaces, sessions, terminals, subscriber lists.
                  Pure: no IO, no await.
    acp.rs        supervisor loop around connect_with, restart with backoff,
                  Agent facade over ConnectionTo<Agent>
    ingest.rs     drains the AcpIncoming channel into State, then publishes
    ops.rs        prompt, load, delete, cancel, answer_permission as spawned
                  tasks
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
  main.rs         builder, dialog plugin, managed Link, window focus hook
  link.rs         owns ur_client::Client; reconnect loop; Desired { watch,
                  subscribed, attached, focus }; forwards events to app.emit
                  and PTY bytes to the terminal Channel
  commands.rs     request(req: Request) -> Response, terminal_input,
                  set_visible, read_attachment
  terminal.rs     terminal ID to Channel<Vec<u8>>
  gui_state.rs    gui.json
  menu.rs         context menus and the folder picker

app/src/
  ipc/            request(), onWatch(), onSession(id), attachTerminal(id),
                  typed by the generated bindings
  store/          watch.ts, sessions.ts as Map<id, SessionState>, terminals.ts
  transcript/     reduce.ts: (SessionState, Event) -> SessionState, pure and
                  unit tested; blocks.ts display types
  components/     Sidebar, Thread, Editor, Permission, TerminalPane, Layout,
                  Tab
  hooks/          useSession(id) with refcounted subscribe and unsubscribe,
                  useTerminal(id)
  keys.ts
```

`ur-client`'s `Client` is a cloneable handle over one writer task and a
pending map keyed by request ID. Every CLI subcommand and the Tauri core use
it unchanged. TypeScript bindings for `protocol.rs` come from `ts-rs`, with
ACP payload fields overridden to the types exported by
`@agentclientprotocol/sdk`, so the webview writes no protocol types by hand.

### Daemon architecture

- One `Arc<Mutex<State>>` with a std mutex, never held across an await.
  Every mutation is a method on `State` that returns the events to publish.
  The caller drops the lock, then pushes to outboxes. Snapshot and subscriber
  registration happen inside one method, which gives the ordering guarantee
  under Wire protocol. An outbox is a bounded `mpsc::Sender<Frame>` used with
  `try_send`; a full outbox closes that connection.
- ACP handlers only forward. The notification handler sends
  `AcpIncoming::Update` and the permission handler sends
  `AcpIncoming::Permission(request, responder)` on an unbounded channel, then
  returns. The SDK runs every handler on one event loop, so handlers do no
  other work. The ingest task is the single writer of ACP data into `State`,
  which gives total order. Replay from `session/load` arrives through the same
  handler and the same `apply_update`.
- The supervisor calls `connect_with`, stores a clone of `ConnectionTo<Agent>`
  in `State`, and awaits a shutdown-or-closed signal. Each connection has a
  generation number carried on every `AcpIncoming` and every op result;
  `State` ignores anything from an earlier generation. On exit the supervisor
  fails `Working` and `NeedsPermission` sessions, clears pending requests,
  bumps the generation, and reconnects with backoff.
- Ops are spawned tasks, and the guard lives in `State` as `session.op`.
  Under the lock: if `op` is set, return busy; otherwise set it and, for a
  prompt, append the user entry and set `Working`. Send the request through
  the connection clone and await it. Under the lock again: clear `op` and
  apply the result. Delete holds `op` through cancel, wait, and delete.
- Pending permission requests hold their SDK `Responder` in
  `Session.pending`; `answer_permission` and cancellation respond through it.
- `Entry`, `Status`, and the snapshot structs are defined in `ur-client` and
  used as-is inside `State`. There is no conversion layer.

### GUI architecture

The GUI is a single-window Tauri app. Its Rust core is the socket client: it
connects to the daemon, retries until the daemon is up, and is the only part
of the app that speaks the wire protocol. The webview never touches the socket.

- One Tauri command, `request`, takes a wire-protocol `Request` and returns
  the daemon's `Response`. The tagged `Request` enum carries the name and
  arguments, so adding a request touches `protocol.rs` and the daemon only.
  The other commands are `terminal_input`, `set_visible`, and
  `read_attachment`.
- Daemon events reach the webview as Tauri events: `watch` events under one
  name, and each subscribed session's events under a name that carries the
  session ID. The webview keeps the sidebar and each subscribed session's
  state in stores outside React, keyed by session ID, so closing a tab keeps
  the transcript and reopening does not refetch. Components read the stores
  through `useSyncExternalStore` or zustand.
- One transcript reducer groups consecutive message and thought chunks,
  combines a user's text and image parts, and updates tool calls by ID.
  A fresh snapshot resets that state and uses the same reducer as live events.
  The reducer is the only code that knows ACP update shapes; components
  render the blocks it derives.
- Terminal output goes through a Tauri IPC `Channel` of raw bytes, one per
  attached terminal, so it is never JSON-encoded. Terminal input goes to the
  core through a `terminal_input` command, which sends a binary `PTY` frame.
- The webview tells the core which sessions are visible in panes through
  `set_visible`. The core combines that with the window's focus, which it
  gets from `WindowEvent::Focused`, and sends `focus` to the daemon as
  described under Session status.
- Native pieces come from Tauri: the dialog plugin for the folder picker,
  `Menu::popup` for context menus, and the window's drag-drop event, which
  gives the core file paths to read and attach to the next prompt. Keyboard
  shortcuts are handled in the webview.
- The core writes the GUI state file described below.

The `useSession` hook installs the session's event listener, then sends
`subscribe`; it unsubscribes when the last component using that session
unmounts. The Link keeps the desired set: watch, subscribed sessions,
terminal attachments with their sizes, and focus. After a disconnect it
reconnects and replays that set. The webview has no reconnect logic:
snapshots replace store state, terminal views restore through the terminal
attachment path, and a `connection` event drives the no-connection empty
state. Requests interrupted by disconnection report an error and are not
automatically resent; the user can inspect the session before trying again.

### Terminals in the daemon

The daemon owns each login shell through `portable-pty` and retains the
terminal state while the GUI is closed. A terminal pane is an xterm.js
`Terminal` with the fit addon. On attachment, the GUI restores the current
screen and resumes live output. `onData` goes through `terminal_input` to the
PTY, and `onResize` goes to `terminal_resize`.

Reliable restoration of running TUI apps is a requirement. Milestone 0 must
start with the Rust `vt100` crate: feed PTY output into `vt100::Parser` while
the GUI is open or closed, then use `Screen::state_formatted()` as the starting
point for the snapshot sent to xterm.js on attachment, followed by live output.
Milestone 0 validates this integration with real TUI apps and establishes the
attachment format used by the rest of the application.

Each terminal holds its `vt100::Parser` and its list of attached outboxes
under one std mutex. The reader thread locks it, feeds the parser, and fans
the bytes out to every attached outbox. Attach locks the same mutex, takes
`state_formatted()` and the size as the snapshot, and adds its outbox. No
output can arrive between the snapshot and the registration.

Detaching or losing the GUI leaves the shell and its applications running.
Close Terminal and Remove Workspace stop the corresponding terminals.

Selecting a terminal that already has a tab activates that tab.

### GUI state

The core keeps the GUI's layout in `$XDG_STATE_HOME/ur/gui.json`, keyed by
socket path: the selected session or terminal, and from milestone 10 the pane
layout as dockview's serialized layout. Panes refer to sessions and terminals
by ID. When the GUI opens, it drops panes whose session or terminal no longer
exists.

### Transport

The daemon listens on a Unix socket, `$UR_SOCKET` or `$TMPDIR/ur.sock`. There
is no TCP listener and no auth code.

### Runtime

The daemon runs on Tokio, because the ACP Rust SDK (`agent-client-protocol`
2.1.0) is async. The supervisor in `daemon/acp.rs` uses `Client.builder()`
with the forwarding handlers for `SessionNotification` and
`RequestPermissionRequest` described under Daemon architecture, and
`connect_with` to an `AcpAgent` built from the configured command and
arguments (see the SDK's `examples/yolo_one_shot_client.rs`). Requests to the
server go through a clone of `ConnectionTo<Agent>` from ordinary Tokio tasks,
awaited with `block_task()`. PTY reads stay on a blocking thread. The app's
core runs the socket connection on Tauri's Tokio runtime.

## Milestones

Each milestone ends with a check you can run. Milestone 0 builds the terminal,
1–4 add agent sessions and the CLI, 5–8 add the agent GUI, 9–10 add workspace
controls for terminals and the pane layout, and 11 adds notifications and
hooks. The early agent GUI and empty-state wireframes show milestone 5. The
other wireframes include controls from later milestones; each check covers
the parts its milestone adds.
Within a milestone, complete the numbered tasks in order. Each task should
leave a working slice that can be reviewed before the next one begins.

### 0. Working terminal

Build the daemon and Tauri app in the project layout above, starting with a
login shell through `portable-pty` and an xterm.js view. Restore terminal state
across GUI disconnects using `vt100::Parser` and `Screen::state_formatted()` as
described above. This is the first working part of ur. Keep it and build the
remaining milestones on top of it.

1. Set up the Cargo workspace, daemon socket, Tauri app, and `ur-client`
   framing needed for one terminal. Open a login shell through `portable-pty`;
   show its output in xterm.js and send input and resize events back to it.
2. Feed PTY output through `vt100::Parser` and establish the terminal
   attachment format: a formatted screen snapshot followed by live output.
   Verify it with a running full-screen TUI while the GUI is attached.
3. Keep the shell and parser alive across GUI disconnects. Reattach and resize
   the same terminal without losing the running application or its screen.

Check with `top` and an editor holding an unsaved buffer. Close and reopen the
GUI while the daemon stays running, including at a different window size.
The application must keep running, render correctly, and accept input; the
editor must retain its unsaved buffer. Get this working before building on it.

### 1. One-shot ACP client

`ur agent-run <workspace> <prompt>` starts the configured server, sends
`initialize`, `session/new`, and `session/prompt`, prints every update as one
line, and asks on stdin when the server requests permission. Retain the
negotiated capabilities for optional features.

Check with Ox: in Ask mode, a prompt that runs `ls` stops at a permission request.
Approving it prints the tool call and the answer, and the command ends with
`end_turn`.

### 2. Daemon hosts ACP sessions

1. Extend the Tokio daemon from milestone 0 with the configured ACP child and
   one connection. Create sessions, send prompts, and retain user prompts and
   ACP updates in memory. Reject permission requests until milestone 3.
2. Add the JSON `subscribe` protocol and the per-session operation guard.
   A subscriber receives a transcript snapshot followed by live updates in
   order; an overlapping prompt returns busy without changing the running
   turn.
3. Add `ur new <path>`, `ur prompt <session> <text>`, and
   `ur read <session> [--follow]` over the socket.
4. Test the session and subscription path with an SDK `Agent.builder()` fake
   agent in the test process, without a model provider. Cover snapshot then
   live with no gaps or duplicates and overlapping prompts. Use labels,
   permission options, and tool inputs different from Ox; include a minimal
   agent with no optional history or image capabilities.

Check: run `ur read --follow` on a session in one terminal and `ur prompt` with
a prompt that needs no shell command in another. The reply streams into the
first terminal, and a second `ur read --follow` started partway through shows
the whole transcript once.

### 3. Status, permissions, and workspaces

1. Add workspaces and their state file. `ur new` now takes a workspace name;
   `watch` lists sessions created during the current server connection under
   their workspace. Add `ur workspace add <name> <path>` and `ur ls`.
   Discovery of saved sessions through `session/list` belongs to milestone 4.
2. Add the session status table, `focus`, and unread tracking. Publish status
   and attention changes to all watching clients.
3. Keep every pending permission request and send it to attached clients.
   Implement first-answer-wins responses and cancellation that answers all
   pending requests with `Cancelled`. Add `ur cancel <session>` and
   `ur approve|deny <session>`. Add `ur workspace rm <name>`, cancelling its
   running turns before removal.
4. Add transcript entries for rejected prompts and turn errors, with the
   status transitions specified above.
5. Add `ur wait <session> [--until attention|idle]`. Test status transitions,
   first answer wins, several pending requests, cancellation, unread and
   focus, rejected prompts, turn errors, and a busy second prompt that leaves
   the first running.

Check: start two sessions in two workspaces from the CLI, run
`ur wait --until attention` on both, and approve one from a second terminal.

### 4. Restore agent history

1. On daemon startup, read the workspaces from the state file and start the
   configured server. When `session/list` is supported, follow its pagination
   for each workspace and add saved sessions to `watch` with their titles and
   last activity. Saved sessions start idle. Without list support, show only
   sessions created during the current connection.
2. Load a saved session when it is first viewed or prompted, if
   `session/load` is supported. Replace its in-memory transcript with the
   replayed entries and publish a fresh session snapshot. A failed load sets
   `Failed` but keeps the session in the sidebar; retry before its next prompt.
3. Handle server exit during a turn: append one turn error, clear pending
   permissions, and restart the server. Reload subscribed sessions where
   supported and leave other saved sessions for lazy load. Preserve the
   interruption error when replacing an affected transcript. Without history
   capabilities, start new sessions after the server restarts.

Check with Ox: kill the daemon partway through a turn and start it again. The session
shows its history up to the last batch that was committed, and a new prompt
works. Then kill the server partway through a turn: the session shows the error and
needs attention, and a new prompt works.

### 5. Agent GUI

One window laid out like Zed's agent panel: a title bar, a workspace panel on
the left, and the selected session or terminal. Add agent threads to the
existing GUI. The GUI talks to the daemon only through
the protocol from milestones 2 and 3, bridged by the core as described under
GUI architecture.

Milestone 5: basic agent GUI.

![Thin GUI](design/thin-gui.png)

1. Connect the Tauri core to the daemon socket, retrying until it is up.
   Bridge `watch`, `subscribe`, `prompt`, and `cancel` through the `request`
   command and events. Install webview listeners before requesting snapshots.
2. Build the workspace panel from `watch`, in workspace creation order and
   session last-activity order. Selecting a session subscribes to it and
   displays its thread. Show the no-connection, no-workspace, and
   no-selection empty states.
3. Add the transcript reducer and thread view: user messages in a box with
   the buffer font, plain agent messages and thoughts, and one titled line
   per tool call. Add the borderless editor with Send and Stop.
4. Save and restore the selected session. On daemon reconnect, restore watch
   and subscriptions, then replace local thread state from fresh snapshots
   so replay does not duplicate entries.

Milestone 5: empty states, before GUI workspace controls are added.

![Empty states](design/empty-states.png)

Check: add a workspace and create a session from the CLI, then prompt it and
read the reply in the GUI. Close the GUI and reopen it: the same session is
selected with the same transcript.
Restart the daemon with the GUI open: it reconnects and replaces its history
without duplicate entries, and sending a new prompt works.

### 6. Attention in the GUI

- Workspace panel: workspaces with sessions that need attention come first,
  each with a count of those sessions. Each session shows its status at
  the right edge of its row: a dot when it needs permission, a spinner while
  it works, and `!` when it failed. Unread sessions are bold.
- The GUI reports focus as described under GUI architecture.
- When a session needs permission, each pending request renders from the
  request's tool call details, merged with any existing tool call of the same
  ID: its supplied title and content, and one row per permission option, with
  its supplied label, an icon from its kind, and a shortcut, followed by
  "Awaiting Confirmation." If no tool row exists yet, render the request after
  the last entry. No subagent detection or ID parsing. Shortcuts answer the
  oldest request.
- A failed turn ends with its error in the thread. A rejected prompt shows
  that error directly beneath the user message.

![Permission request](design/permission.png)

Check: the scenario at the top of this document, with the sessions created
from the CLI and everything else done in the GUI.

### 7. Thread rendering

- Agent messages render as Markdown with `react-markdown` and `remark-gfm`,
  with a copy button under each response that copies its Markdown source.
- Thoughts are a collapsed "Thinking" row. Tool calls of ACP kind `execute`
  use a filled "Run Command" block with the supplied title. Other tool calls
  use an icon for the kind and the supplied title. `raw_input` is arbitrary
  JSON, not a portable command schema; ur does not depend on a `command` key
  or recognize server-specific tool names.
- Clicking a Thinking row or a tool call expands its content: the thought's
  text or the tool call's text content. Edits show their outcome text like
  other tool calls. A Run Command block expands to its output.
- A rejected prompt shows its error under the user message.

Tool presentation follows ACP's
[tool call fields](https://agentclientprotocol.com/protocol/v1/tool-calls),
not the server's tool implementation.

![Main window](design/main-window.png)

![Expanded thread](design/thread.png)

Check: use a session with thinking, edits, and shell commands, then send a
prompt the server rejects. Each renders as in the wireframes, and the thinking and
tool rows expand and collapse.

### 8. Session management and editor

1. Add workspace controls: `+` opens the dialog plugin's folder picker and
   adds a workspace named after the folder. A native workspace context menu
   offers New Session and Remove Workspace, with the required confirmation.
   Add the thread title and `+` to create a session in that workspace.
2. Implement `delete_session` through the daemon and GUI when the server
   advertises it. Add Delete to the session's native context menu. Confirm,
   then cancel any active turn, wait for it to end, and call `session/delete`
   as described under Sessions, tabs, and workspaces.
3. Pass through `available_commands_update`, config options, and usage data.
   Show slash commands when the user types `/`, add one picker for each
   supplied config option with `set_config_option`, and show usage indicators
   when reported, with available tokens and cost on hover.
4. When image prompts are supported, accept image content through the prompt
   path. Use the window's drag-drop event to attach an image file to the next
   prompt; the core reads the file. Route server rejection through the
   ordinary prompt-error path without hard-coding Ox's image limits.

![Menus](design/menus.png)

![Editor](design/editor.png)

Check: the scenario at the top of this document, done entirely in the GUI.

### 9. Workspace terminal controls

Extend the existing terminal implementation with workspace ownership and
sidebar controls. Carry the terminal title in `watch`; use the shell's name
until a program sets a title. Terminals are listed under their workspace in
the sidebar. The workspace menu gains New Terminal, which starts a login
shell in the workspace directory. Selecting it shows the xterm.js pane under
a header with its title. Right-clicking a terminal in the sidebar offers
Close Terminal, which stops it.

![Terminal](design/terminal.png)

Check: create terminals in two workspaces. Leave `npm run dev` running while
closing and reopening the GUI. Selecting its terminal restores the view;
Close Terminal stops it.

### 10. Pane grid

The session area becomes a dockview layout that holds agent sessions and
terminals side by side. Each dockview group is a pane, and each panel is a
tab.

A tab is a custom tab component that shows its kind (agent or terminal) and,
for an agent, its status; hovering a tab shows its Close Tab button, which
leaves its session or terminal running. The group's header actions hold
`+`, which opens New Session and New Terminal in the active tab's workspace
(or the selected sidebar workspace if the pane is empty), and the
split button, which opens Split Right, Left, Up, and Down, each with a
shortcut. Dragging a tab is dockview's own: dropping it on a pane's edge
splits, and dropping it on a pane's center moves the tab there. Choosing a
session or terminal in the sidebar opens it in the active pane; a terminal
with an existing tab activates that tab. The GUI focuses every session visible
in a pane and saves the layout as described under GUI state.

1. Replace the single session area with dockview groups and panels for agent
   sessions and terminals. Add custom tabs with kind, agent status, and Close
   Tab; closing a tab leaves its session or terminal running.
2. Add group header actions for New Session, New Terminal, and splits in four
   directions with shortcuts. Use dockview's tab dragging to split or move
   tabs between panes.
3. Open a sidebar selection in the active pane; activate an existing terminal
   tab instead of opening a duplicate. Report focus for every visible session
   and save and restore the dockview layout, dropping references to sessions
   or terminals that no longer exist.

![Panes](design/panes.png)

Check: a terminal running `npm run dev` next to an agent session. Close the GUI,
reopen it, and both are restored in the same layout.

### 11. Notifications and hooks

- The daemon posts a macOS notification when a session starts to need
  attention and no client has it focused, so notifications arrive while the
  GUI is closed.
- `on_event` in `$XDG_CONFIG_HOME/ur/config.toml` (under `~/.config` when the
  variable is unset): a command the daemon runs for each status change, with
  the event as JSON on stdin.

Check: start a long prompt, switch to another app, and get a notification when
it finishes.
