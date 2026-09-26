# Multiple ACP servers

## Goal

Run sessions from several named ACP servers in the same daemon and workspace. The workspace menu and
new menu offer one entry per configured server alongside New Terminal. Server settings let the user
add, name, edit, and remove servers.

Follow `AGENTS.md`, `docs/agents/architecture.md`, `docs/agents/code-style.md`,
`docs/agents/glossary.md`, and `docs/agents/testing.md`; update the single-server descriptions
listed below as part of the implementation.

## Related code

- `crates/ur/src/config.rs`, `daemon/mod.rs`, and `daemon/acp.rs` — one launch configuration,
  supervisor, and startup readiness signal currently serve the whole daemon.
- `crates/ur/src/daemon/state.rs` and `daemon/ops.rs` — the ACP connection, capabilities, and
  generation are global, and sessions are found by ACP session ID alone.
- `crates/ur-client/src/protocol.rs` — `set_server`, `new_session`, server state, capabilities, and
  every session reference cross the socket here.
- `app/src/actions.ts`, `components/Sidebar.tsx`, `components/Layout.tsx`, and `App.tsx` — creation
  menus, shortcuts, settings, and capability-dependent controls assume one server.
- `app/src/components/ServerSetup.tsx` and `store/watch.ts` — one server's form and connection
  state.
- `app/src-tauri/src/link.rs` — replays subscriptions and focus after daemon reconnect and routes
  watch events. Its replay currently relies on saved sessions being discovered before serving.
- `crates/ur/src/daemon/tests.rs` and `app/e2e/harness.ts` — single-server fixtures. Independent
  fake servers already produce overlapping IDs such as `fake-1`, which exercises session separation.

## Decisions

### Configuration and settings

Replace the config file's `server` object with an ordered `servers` array. Each entry contains an
immutable `id`, an editable `name`, `command`, and `args`. Generate the ID in the daemon with a UUID
when adding a server; persist it so renaming and restarting preserve session identity. Add `uuid`
with its `v4` feature as a workspace dependency used by `ur`; the lockfile already contains it.

Names must be nonempty and unique; menu order follows configuration order. Commands and arguments
retain their existing meaning. GUI additions and edits retain the absolute executable requirement.
Validate config-file IDs and names for uniqueness and return clear configuration errors. No config
migration is needed; document the new shape and let the existing setup flow replace an invalid old
configuration explicitly when the user saves a server.

Keep `ServerSetup.tsx` as the settings screen, with a list of servers and a selected server's form.
Each row shows its name and connection status or error. Add Server opens a blank form. Save updates
the selected server or adds one; a name-only change does not restart its process. A command or
argument change restarts only that server. Editing a launch configuration assumes it still refers to
the same server's saved history; a different server is added as a separate entry.

Remove asks for confirmation, explaining that its running sessions in ur will stop. It stops that
supervisor, resolves outstanding requests, and removes its sessions and tabs from ur, without
calling `session/delete`. Other servers, workspaces, and terminals remain available. An empty server
list is valid. Initial setup opens automatically with no configured servers but can be dismissed to
use terminals. Adding or repairing one server must not require the others to connect.

### Connections and sessions

Use one supervisor and ACP connection per configured server, shared by that server's sessions across
workspaces. Keep the existing shared `State` mutex. Store connection state, capabilities, and
generation per server; keep workspaces, terminals, watchers, and permission request numbering
shared.

A session belongs to exactly one server. Its ACP session ID remains opaque and is sent unchanged to
that server. On the wire, use a separate stable session key, represented as a string containing the
JSON serialization of `[server_id, acp_session_id]`. A Rust `SessionKey` newtype distinguishes it
from ACP's `SessionId`. Store the server ID and ACP session ID on the daemon's session; lookup by
session key does not need to parse it. The webview continues treating session references as opaque
strings in stores, tabs, focus, and saved layouts. Identical ACP session IDs from different servers
therefore remain distinct across daemon restarts and server renames.

Capture server ID and generation on every connection handler and asynchronous result, including
initialize, list, new, config changes, notifications, permissions, and disconnect handling. Reserve
the generation before a connection attempt and invalidate it when retiring that connection. Ignore
old results for state mutation, while still settling their outstanding daemon requests; answer
obsolete permission requests with cancellation. Replacing or removing a server must not allow an old
handler to update a new connection or recreate removed sessions.

List saved sessions for every workspace on each server that supports listing. Adding a workspace
lists it independently on all connected servers that support listing. Route prompt, load, cancel,
delete, config changes, and workspace-removal cancellation through each session's own connection.
Server exit fails and unloads only that server's sessions and reloads only its subscribed sessions
after reconnection. Capabilities are never combined across servers.

Start all configured supervisors concurrently. Preserve the existing initial readiness rule: begin
serving after each first attempt has connected and listed, failed, or reached the existing timeout.
The attempts run concurrently, so their timeouts do not add together. This preserves layout and
subscription restoration without introducing partially discovered startup snapshots. Later restarts
and settings changes do not block the socket or other servers.

### Menus and shortcuts

Both creation menus replace New Session with `New Session — <server name>` for every configured
server, followed by New Terminal. Include disconnected servers as disabled entries. The workspace
menu retains Remove Workspace. Selecting an entry creates a session on that server in the menu's
workspace and opens it in the intended pane.

With several configured servers, Command+N or Ctrl+N opens the server choices in the current
workspace; no default server is stored. With exactly one configured server, it creates a session
directly when connected. With none, it opens server settings. Preserve the existing active-workspace
and first-workspace selection rule, and the new-terminal shortcut.

Show the server name as secondary text on session rows and in session tab tooltips so the user can
identify a session's server without changing its session title. Keep the workspace's combined
session ordering. Image support and Delete availability use the owning server's current
capabilities. Connection errors name the affected server and open that server's settings.

The one-shot client accepts `--server <name>`. It selects the only configured server when the flag
is omitted; with several, it requires the flag and lists the available names in its error.

## Naming

Existing terms retain the meanings in `docs/agents/glossary.md`, with the server, ACP connection,
generation, and server state definitions changed from daemon-wide to per server.

- **Server ID** — the immutable identifier of one configured server, persisted as `ServerConfig.id`
  and carried in server state and session summaries. It is independent of the editable server name.
- **Server name** — the user-chosen label shown in settings and creation menus, `name` in server
  configuration and server state.
- **ACP session ID** — the opaque server-assigned `SessionId`, used only at the ACP boundary.
- **Session key** — the daemon client's stable reference to a session on one server, `SessionKey` in
  Rust and a string in TypeScript. Existing wire fields named `session` carry this value.

## Test plan

- Config tests own ordered round trips, stable IDs through rename, rejection of duplicate IDs or
  names, and one-shot server selection for zero, one, or several configured servers.
- Protocol tests own session-key separation and stability, including ACP IDs containing punctuation
  and quotes. Generated bindings must retain string session references.
- Daemon tests use two fake servers with independent histories and identical ACP session IDs. Cover
  independent transcripts, prompts, config changes, permission answers, cancellation, deletion,
  focus, and unread state, with one owning test per guarantee.
- Daemon tests cover saved discovery on both servers at startup and workspace addition; independent
  restart and subscribed reload; differing history and delete capabilities; and workspace removal
  across both servers. A failed or stalled first attempt must obey the startup timeout.
- Daemon tests cover removal and launch edits while operations are pending, and late results from a
  retired connection. They cannot change another server or recreate a removed session. Saving a
  config that fails to write must leave the running configuration intact.
- Watch reducer tests own per-server state replacement and capability clearing after one server
  disconnects, plus the shared ordering and attention behavior with mixed-server sessions.
- End-to-end tests own choosing either server from both creation menus, opening the result in the
  correct pane, and the multiple-server Command+N/Ctrl+N chooser. Keep the single-server shortcut
  guarantees. Menu tests inspect the menu entries and execute their registered actions through a
  test-only native-menu IPC interception in the harness: capture `plugin:menu|new` options and use
  the selected item's channel handler when `plugin:menu|popup` is called. Keep real menu
  construction and daemon requests; do not add production testing commands or a second menu
  implementation.
- End-to-end tests own adding, renaming, correcting, and removing a server through settings;
  identifying each session's server; restoring two servers' tabs and transcripts after GUI and
  daemon restarts; and continuing another session and a terminal while one server is unavailable.
- End-to-end tests use fake servers with different advertised image and delete capabilities to
  verify that the editor and session menu follow the session's server. Update the guarantee list and
  the existing setup tests to match their new scope.

## Implementation plan

1. In `Cargo.toml`, `Cargo.lock`, `crates/ur/Cargo.toml`, and `crates/ur/src/config.rs`, add stable
   server IDs and the ordered server configuration. Keep atomic file replacement, and place config
   validation and server selection tests beside the config code. In `crates/ur/src/main.rs` and
   `crates/ur/src/one_shot.rs`, add the one-shot server selection described above.
2. In `crates/ur-client/src/protocol.rs`, introduce `SessionKey`, add a server ID to
   `SessionSummary`, and require `server` on `NewSession`. Replace `SetServer` with `AddServer`,
   `UpdateServer`, and `RemoveServer`; return the assigned ID in `ServerAdded`. Put each server's
   ID, name, command, args, connection status, error, and optional capabilities in `ServerState`.
   Replace global server/capability fields with ordered `servers` and `config_error` in
   `WatchSnapshot`. Replace the two separate change events with `ServersChanged`, carrying that same
   complete server list and config error. This also covers additions, removals, and recovery from a
   bad config file. Regenerate `app/src/ipc/bindings/Request.ts`, `Response.ts`, `Event.ts`,
   `ServerState.ts`, and `SessionSummary.ts`, and create `SessionKey.ts` in that directory.
3. In `crates/ur/src/daemon/mod.rs`, make `ServerControl` own the ordered configuration and
   supervisor tasks by server ID. Serialize additions, edits, and removals; persist before applying
   them. Start supervisors concurrently and retain the bounded initial readiness wait. Update
   `daemon/server.rs` to dispatch the new server requests and selected-server session creation.
4. In `crates/ur/src/daemon/state.rs`, move connection state and generations under their servers,
   add session keys and ownership, and scope all operations and cleanup accordingly. Update
   `daemon/acp.rs` and `daemon/ops.rs` to capture connection identity, preserve ACP IDs in requests,
   reject obsolete results, list per server, and route session operations. Resolve pending
   subscriptions and request responses when a server is removed; publish session removal events so
   existing stores and tabs discard those sessions. A launch edit uses the normal scoped exit and
   reconnect path, while a name edit only publishes server state.
5. In `crates/ur/src/daemon/tests.rs`, extend the existing fixtures with named server launches,
   separate histories, and individual server termination; retain convenient single-server setup for
   existing tests. Add the daemon guarantees above. In `crates/ur-fake-server/src/lib.rs` and
   `main.rs`, allow test-selected advertised capabilities for the differing-capability cases while
   retaining the existing scripts and default behavior.
6. In `app/src-tauri/src/link.rs` and `app/src/ipc/index.ts`, route the new watch event and carry
   session keys through existing requests. In `app/src/store/watch.ts` and `watch.test.ts`, replace
   the singleton with the ordered server list and config error. Update the `SessionSummary` fixture
   in `app/src/layout.test.ts`; tab params and layout algorithms can continue using opaque strings.
7. In `app/src/components/ServerSetup.tsx` and `app/src/App.tsx`, implement the server list and
   selected-server form, removal confirmation, dismissible empty setup, and per-server errors. Track
   the server being saved so an already-connected different server cannot complete its save flow. In
   `app/src/actions.ts`, share server-choice entries between menus and add an explicit server
   argument to `newSession`. Update `App.tsx` for the multiple-server shortcut choice.
8. In `app/src/components/Sidebar.tsx`, use each session's server for Delete availability and its
   secondary server name. In `components/Tab.tsx`, include that name in the session tooltip. In
   `components/Layout.tsx`, pass server choices to the pane menu and the session's own capabilities
   to `Editor`. The editor's existing capability input remains sufficient.
9. In `app/e2e/harness.ts`, support several configurations, independent history files, selecting a
   server for setup requests, waiting for a specific server, and the native-menu interception.
   Update `app/e2e/server-setup.test.ts` and `panes.test.ts` for the changed settings and shortcut
   behavior. Add `app/e2e/multiple-servers.test.ts` for the new end-to-end guarantees.

## Documentation updates

- `README.md` — explain adding named servers, choosing one from creation menus, the new shortcut
  behavior, config shape, and one-shot selection.
- `AGENTS.md` — update the affected file descriptions and map the new end-to-end test file.
- `docs/agents/architecture.md` — replace single-server assumptions in configuration, connection
  ownership, session identity, startup, history discovery, wire protocol, settings, and shortcuts.
- `docs/agents/glossary.md` — add the naming above and update server state, session summary, watch,
  workspace menu, and new menu definitions.
- `docs/agents/testing.md` — update fixture descriptions and the end-to-end guarantees above.
- `docs/agents/todo.md` — record multiple-server support and link this plan.
