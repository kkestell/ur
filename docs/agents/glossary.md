# Glossary

## Naming

- Use server for the ACP agent process the daemon launches. Reserve agent for SDK type names
  (`Agent`, `ConnectionTo<Agent>`), test agents, and the agent side of the UI (agent session, agent
  tab, agent message) where it contrasts with terminals.
- Qualify client as ACP client or daemon client whenever the surrounding text does not make it
  obvious. `ur_client::Client` is a daemon client; the SDK's `Client.builder()` builds the ACP
  client.
- Qualify connection as ACP connection or socket connection whenever the surrounding text does not
  make it obvious.
- Qualify protocol as ACP or wire protocol. Never write ACP protocol.
- Session always means an ACP session. Say terminal, never terminal session.
- Use transcript for the daemon's in-memory entries and thread for the GUI's view of one session's
  transcript. History appears only as saved history, the server's durable copy.
- Say session title, terminal title, or tool call title. Never write an unqualified title, and never
  write thread title for the session title.
- Say sidebar, not workspace panel.
- Qualify attachment as terminal attachment or image attachment.
- Attach refers only to terminals. Sessions are watched or subscribed.
- Say config option only for an ACP session config option. `config.json` is the config file.

## Names across boundaries

| Domain                     | ACP                                                     | ur                             |
| -------------------------- | ------------------------------------------------------- | ------------------------------ |
| server                     | agent                                                   | `Agent`, `ConnectionTo<Agent>` |
| workspace path             | `cwd`                                                   | —                              |
| session title              | `title` in `session/list` and `session_info_update`     | session title in `watch`       |
| last activity              | `updatedAt` in `session/list` and `session_info_update` | `SessionSummary.updated_at`    |
| user message, replayed     | `user_message_chunk`                                    | ACP update entry               |
| user message, sent         | `session/prompt` content                                | user prompt entry              |
| thought                    | `agent_thought_chunk`                                   | "Thinking" row                 |
| stop reason                | `stopReason`                                            | `Idle { last_stop }`           |
| pending permission request | `session/request_permission`                            | `NeedsPermission { requests }` |
| cancellation               | `session/cancel`                                        | `cancel(session)`              |
| config option              | `configOptions`, `config_option_update`                 | `set_config_option`            |
| slash command              | `available_commands_update`                             | —                              |
| usage indicator            | `usage_update` `used`, `size`, and `cost`               | —                              |
| session operation          | —                                                       | op, guarded by `session.op`    |

## Terms

### Processes and configuration

- **ur**: The ACP client this project builds: a daemon and a GUI.
- **ACP**: Agent Client Protocol, the JSON-RPC interface between ur and a server. It is the
  contract; server-specific names have no special meaning to ur.
- **Server**: The ACP agent process the daemon launches from the config file. There is one per
  daemon. Ox is the server used for development and end-to-end checks, not part of ur's
  architecture.
- **Ox**: The ACP server at `~/projects/ox` that ur is developed and tested against. Its tools,
  limits, database, and notification timing are not client requirements.
- **Test agent**: A server built with the SDK's `Agent.builder()` inside a test process. It
  exercises ACP without a model provider or Ox.
- **Fake server**: The scripted server in `crates/ur-fake-server`. Its binary is what the daemon
  launches in end-to-end tests, and its library is the test agent in the daemon's tests. The
  prompt's text chooses its script: `hold`, `tool`, `tools` (two permission requests at once),
  `reject` (a rejected prompt), `fail` (a turn error), `title` (a session title), `unloadable` (a
  session whose loads fail), `pace` (a `config_option_update`), `usage` (a `usage_update`), or a
  reply. Its saved history is a `SavedHistory`, which the daemon's tests share between fake servers.
- **Daemon**: The long-running `ur daemon` process. It owns the ACP connection, session statuses,
  in-memory transcripts, pending permission requests, and terminals, and serves daemon clients on
  the socket. The GUI starts its bundled daemon sidecar when no daemon is listening.
- **Server state**: The configured ACP server executable and arguments, whether its connection is
  ready, and its latest connection error. Watch sends it in the snapshot and on each change.
- **One-shot client**: `ur agent-run`, which launches the server and speaks ACP directly for one
  prompt without a daemon.
- **Daemon client**: The GUI or a test client connected to the daemon socket. Every daemon client
  reads the same session status.
- **GUI**: The single-window Tauri app. It consists of the core and the webview.
- **Core**: The GUI's Rust side in `app/src-tauri`. It is the GUI's only daemon client and the only
  part of the GUI that speaks the wire protocol.
- **Webview**: The GUI's React and TypeScript frontend in `app/src`. It reaches the daemon only
  through the core.
- **Link**: The core's owner of `ur_client::Client`. It reconnects to the daemon and replays the
  desired set after a disconnect.
- **Desired set**: What the Link restores after reconnecting: watch, subscribed sessions, terminal
  attachments with their sizes, and focus, which is the visible sessions while the window has focus.
  It is `Desired` in code.
- **Config file**: `$XDG_CONFIG_HOME/ur/config.json`, under `~/.config` when the variable is unset.
  It holds the server command and arguments and, from milestone 11, `on_event`. The daemon and
  one-shot client share it.
- **State file**: `$XDG_STATE_HOME/ur/state.json`, under `~/.local/state` when the variable is
  unset. It holds workspace names and paths, and nothing about sessions.
- **GUI state file**: `$XDG_STATE_HOME/ur/gui.json`, written by the core and keyed by socket path.
  It holds the layout.
- **Capabilities**: What the server advertised during `initialize`. ur calls an optional method only
  when it is advertised and advertises only client capabilities it implements.
- **History capabilities**: The server's `session/list` and `session/load` support. Without them, ur
  supports only sessions created during the current ACP connection.

### Daemon internals

- **ACP connection**: The daemon's one connection to the server, shared by every session in every
  workspace. It is `ConnectionTo<Agent>` in code.
- **Supervisor**: The daemon task in `daemon/acp.rs` that launches the server, holds the ACP
  connection, and reconnects with backoff when the server exits.
- **Generation**: A number identifying one ACP connection. It is carried on every op result, and
  `State` ignores anything from an earlier generation.
- **`State`**: The daemon's in-memory workspaces, sessions, terminals, and subscriber lists behind
  one std mutex. Every mutation is a method that queues its events on the affected outboxes while
  the lock is held, and does no IO.
- **Outbox**: A bounded `mpsc::Sender<Frame>` for one socket connection. A full outbox closes that
  connection.

### Workspaces and sessions

- **Workspace**: A name and a workspace path, stored in the state file. Its sessions and terminals
  appear under it in the sidebar.
- **Workspace path**: The absolute path of a workspace, made absolute when the workspace is added
  and stored as is. Every `session/new`, `session/load`, and `session/list` call for the workspace
  passes that exact string as `cwd`.
- **Session**: An ACP session, identified by a server-assigned session ID. ur treats the ID as
  opaque.
- **Saved history**: The server's durable copy of a session. The daemon saves no transcripts.
- **Saved session**: A session returned by `session/list`. It starts `Idle` and unloaded.
- **Loaded session**: A session whose transcript the daemon holds, either because the daemon created
  it during the current ACP connection or because `session/load` succeeded.
- **Load**: A `session/load` call that rebuilds a session's transcript from replay. Subscribing to
  or prompting an unloaded session starts one, and so does a server restart for each unloaded
  session with subscribers. It replaces the previous transcript only on success.
- **Replay**: The ACP updates the server sends during `session/load`. They go through the same
  handler and `apply_update` as live updates.
- **Session title**: The label of a session from `session/list` or `session_info_update`. A session
  without one shows as "New session".
- **Last activity**: The session's most recent activity time from `session/list` or
  `session_info_update`.
- **Turn**: One `session/prompt` call, from sending it until it returns a stop reason or an error.
- **Stop reason**: The ACP reason a turn ended normally, kept as `last_stop` in `Idle`.
- **Session operation**: One prompt, load, or delete running for a session. At most one runs per
  session at a time. It is an op in code.
- **Operation guard**: The marker that reserves a session for one session operation. It is
  `session.op` in `State`. Delete holds it through cancel, wait, and `session/delete`.
- **Busy**: The rejection a request gets when the session's operation guard is already held. It
  changes neither the transcript nor the status.
- **Waiting delete**: A `delete_session` request that cancelled a running turn, recorded in the
  prompt's operation guard until the turn ends and `session/delete` is sent.

### Transcripts

- **Transcript**: The daemon's in-memory, ordered list of transcript entries for one loaded session.
  Rejected prompts and turn errors exist only here and are gone after a daemon restart.
- **Transcript entry**: An ACP update entry, a user prompt entry, or a turn error entry. It is
  `Entry` in code.
- **ACP update entry**: A `SessionUpdate` the daemon received, stored unchanged.
- **User prompt entry**: The content of a `session/prompt` the daemon sent, appended when the prompt
  is sent.
- **Turn error entry**: The message from a JSON-RPC error returned by `session/prompt`, or from the
  server exiting during a turn. It is the only place a turn's error is stored.
- **Rejected prompt**: A prompt the server answers with a JSON-RPC error before the turn starts, for
  example because the input is too large or the model does not accept images. Its turn error entry
  follows its user prompt entry.
- **User message**: The user's text and image parts as the thread shows them, from a user prompt
  entry or, after a load, from replayed `user_message_chunk` updates.
- **Agent message**: Text from consecutive `agent_message_chunk` updates, rendered as Markdown.
- **Thought**: Text from consecutive `agent_thought_chunk` updates, rendered as a collapsed
  "Thinking" row. Clicking the row shows the text.
- **Tool call**: An ACP tool call, identified by its tool call ID and updated by `tool_call_update`.
  ur uses only ACP's tool call fields, never the server's tool names or `raw_input` keys.
- **Tool call title**: The title the server supplies for a tool call.
- **Tool kind**: The ACP category of a tool call. ur picks an icon from it and renders `execute` as
  a Run Command block.
- **Tool call status**: The ACP state of a tool call: `pending`, `in_progress`, `completed`, or
  `failed`.
- **Tool call content**: The ACP `ToolCallContent` items of a tool call: text content, diffs, and
  terminal content. It is the tool call block's `content` in code, rendered by
  `ToolCallContentView`.
- **Run Command block**: The filled block for a tool call of kind `execute`, showing its tool call
  title and expanding to its output.
- **Expanded**: A Thinking row or tool call showing its content. It is `expanded` in code.
- **Transcript reducer**: The pure webview function in `transcript/reduce.ts` that turns a session
  snapshot and session events into display blocks. It is the only webview code that knows ACP update
  shapes.

### Status and attention

- **Session status**: The daemon-owned state of a session: `Idle`, `Working`, `NeedsPermission`, or
  `Failed`. It is `Status` in code.
- **Unread**: A per-session flag set when a turn ends or fails while no daemon client has the
  session focused, and cleared when one focuses it or sends it a prompt.
- **Needs attention**: True of a session that is `NeedsPermission`, `Failed`, or unread. The sidebar
  shows a count beside a workspace with sessions needing attention.
- **Focus**: The set of sessions a daemon client is showing, sent with `focus(sessions)`. The GUI
  focuses every visible session while its window has focus and nothing otherwise. A daemon client's
  focus clears when it disconnects.
- **Visible session**: A session shown as the active tab of a pane, reported by the webview through
  `set_visible`.

### Permissions

- **Pending permission request**: A `session/request_permission` the server sent and the daemon has
  not answered. The daemon holds its `Responder` in `Session.responders`, and a session can have
  several, oldest first. Each has a request ID: the daemon's `u32` number for it, counting from 1
  across all sessions.
- **Permission option**: One choice in a permission request, with its server-supplied label and
  kind: `allow_once`, `allow_always`, `reject_once`, or `reject_always`.
- **First answer wins**: The rule that the first daemon client to answer a pending permission
  request decides it; the daemon then tells every daemon client the request is resolved.
- **Resolved**: A pending permission request that was answered, cancelled, or cleared because its
  turn ended or the server disconnected.
- **Cancellation**: Sending `session/cancel` and answering every pending permission request for the
  session with `Cancelled`. The session stays `Working` and busy until the prompt returns.
- **Shortcut**: The key combination for a permission option kind, which answers the selected
  session's oldest pending request with its first option of that kind.

### Config options, commands, and usage

- **Config option**: An ACP session config option from `configOptions` or `config_option_update`.
  The GUI shows one picker per option, and nothing about models, effort, or modes is hard-coded.
- **Slash command**: A command from `available_commands_update`, offered when the user types `/`.
- **Usage indicator**: The GUI display of the server's `usage_update`, with available tokens and
  cost on hover.
- **Image attachment**: An image file dropped on the editor, read by the webview, and sent as image
  content in the next prompt. It requires the server's image prompt capability. It is
  `ImageAttachment` in code.

### Wire protocol

- **Wire protocol**: The protocol between the daemon and its daemon clients on the Unix socket at
  `$UR_SOCKET` or `$TMPDIR/ur.sock`.
- **Frame**: A one-byte tag, a big-endian `u32` length, and a payload. A `JSON` frame carries a
  request, response, or event; a `PTY` frame carries a terminal ID and raw terminal bytes.
- **Request**: A JSON message from a daemon client with an ID, answered by one response with the
  same ID.
- **Event**: A JSON message the daemon pushes to a daemon client. Events carry ACP schema types
  unchanged.
- **Watch**: The request that registers a daemon client for sidebar events: workspaces, their
  session summaries and terminal summaries, server state, and capabilities. Pending permission
  requests arrive in the session status.
- **Session summary**: One session as watch shows it: its session ID, workspace, session status,
  unread flag, session title, and last activity. It is `SessionSummary` in code.
- **Terminal summary**: One terminal as watch shows it: its terminal ID, workspace, and terminal
  title. It is `TerminalSummary` in code.
- **Subscribe**: The request that registers a daemon client for one session's content: its
  transcript and config options.
- **Snapshot**: The current state sent before live events: `WatchSnapshot` for watch,
  `SessionSnapshot` for subscribe, and a screen snapshot for a terminal attachment. The daemon takes
  the snapshot and registers the daemon client under one lock, so nothing is missed or duplicated.
- **Session snapshot**: A subscribed session's transcript and config options. A load sends a fresh
  one, which replaces the daemon client's local state.

### Terminals

- **Terminal**: A login shell the daemon runs in a workspace directory through `portable-pty`, with
  its `vt100::Parser` and attached outboxes. It lasts until closed, its workspace is removed, or the
  daemon exits.
- **Terminal title**: The title a program sets for a terminal, or the shell's name until one does.
- **Terminal attachment**: A daemon client receiving one terminal's screen snapshot and live output,
  started with `attach_terminal`. Detaching leaves the terminal running.
- **Screen snapshot**: The terminal's `Screen::state_formatted()` and size, taken under the
  terminal's mutex when attaching.
- **Terminal attachment format**: A screen snapshot followed by live output, established in
  milestone 0.

### GUI layout

- **Sidebar**: The left side of the window, listing workspaces with their sessions and terminals.
- **Thread**: The GUI view of one session's transcript, with the editor below it.
- **Editor**: The borderless prompt input under a thread, with the command list, image attachment
  chips, the usage indicator, config pickers, and Send or Stop.
- **Workspace menu**: The native context menu of a workspace row: New Session, New Terminal, and
  Remove Workspace….
- **Session menu**: The native context menu of a session row: Delete…, shown when the server
  advertises `session/delete`.
- **Terminal menu**: The native context menu of a terminal row: Close Terminal. It is
  `showTerminalMenu()` in code.
- **Command list**: The list of matching slash commands above the editor while typing `/`.
- **Config picker**: The editor control for one config option: a list of a select option's values,
  or a toggle for a boolean option. It is `ConfigPicker` in code.
- **Pane**: A dockview group holding tabs. The active pane receives sidebar choices.
- **Tab**: A dockview panel for one agent session or terminal. Closing it changes only the layout.
  `Tab` is also its tab component.
- **`TabItem`**: The session or terminal one tab shows: `{ type: "session", session }` or
  `{ type: "terminal", terminal }`. It is the panel's params.
- **Close Tab**: The button a tab shows on hover.
- **New menu**: The native menu from a pane's `+`: New Session and New Terminal, in the workspace of
  the pane's active tab. It is `showNewMenu()` in code.
- **Layout**: The dockview arrangement of panes and tabs, saved in the GUI state file. `Layout` is
  the component that holds `DockviewReact`.
- **Selection**: The session or terminal of the active tab, highlighted in the sidebar.
- **Terminal row**: A sidebar row for one terminal, listed under its workspace after the sessions,
  showing the terminal icon and terminal title.

### Notifications and hooks

- **macOS notification**: The notification the daemon posts when a session starts to need attention
  and no daemon client has it focused.
- **Hook**: The `on_event` command in the config file, run by the daemon for each session status
  change with the change as JSON on stdin. It is unrelated to wire protocol events.

### Testing

- **End-to-end suite**: The WebDriver tests in `app/e2e/`, run by `pnpm -C app e2e` against the
  built app. Each test owns one guarantee from the end-to-end guarantee list.
- **End-to-end guarantee list**: The closed list of guarantees in `testing.md` that the end-to-end
  suite owns.
- **Ad-hoc check**: A throwaway script outside the repository that uses `app/e2e/harness.ts` to
  check behavior. It is never committed.
