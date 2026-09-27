# ur architecture

ur is a desktop client for ACP agents. A long-running daemon holds the agent connections, the
sessions, and the terminals. The GUI is a view onto the daemon: closing it stops nothing, and
reopening it picks up where it left off.

## Components

- **ACP servers**: agent processes the daemon launches from the config file. Each is a separate
  process, and any ACP-compatible server works without changes to ur.
- **Daemon** (`crates/ur`): runs every configured server, and owns workspaces, session state,
  permission requests, and terminals. It serves any number of clients over a local Unix socket.
- **Wire protocol** (`crates/ur-client`): the socket's message types and an async client. It is the
  only definition of the protocol. The webview's TypeScript types are generated from it.
- **GUI core** (`app/src-tauri`): the Tauri app's Rust side. It is the GUI's only socket client,
  starts the bundled daemon when none is running, and reconnects after the daemon restarts.
- **Webview** (`app/src`): renders what the core forwards and sends the user's requests through it.
  It never touches the socket.
- **Fake server** (`crates/ur-fake-server`): a scripted ACP server for tests.

## Boundaries

### ACP

The ACP spec is the contract. ur gives no special meaning to server names, tools, models, modes, or
IDs, and has no server-specific code. Optional features follow the capabilities each server
advertises, and ur advertises only the client capabilities it implements. ACP types pass through the
daemon and the wire protocol unchanged, and one reducer in the webview is the only code that
interprets them for display.

### Socket

The daemon listens on a local Unix socket with no authentication. Frames carry either JSON messages
or raw terminal bytes, so terminal output is never JSON-encoded. Clients send requests and receive
responses and events. A client that watches or subscribes gets a snapshot followed by every later
change in order. A client that cannot keep up is dropped rather than slowing the daemon.

### GUI core and webview

The core owns the connection and the set of things the GUI wants: watch, subscriptions, terminal
attachments, and focus. After each reconnect it replays that set, and the fresh snapshots replace
the webview's state, so the webview has no reconnect logic.

## Ownership

- **Saved history belongs to the ACP server.** The daemon keeps transcripts only in memory and
  rebuilds them with `session/load` when the server supports it.
- **The daemon owns live session state**: each session's status, unread flag, and pending permission
  requests. Every client sees the same values.
- **The config file** holds the configured servers. **The daemon's state file** holds workspaces.
  **The GUI state file** holds the layout. Each has one writer.

## Decisions

- **One ACP connection per server.** A server's sessions share its connection across workspaces.
  Each server has its own supervisor, which restarts it after an exit, so one server failing affects
  only its own sessions.
- **Session identity is the server plus the ACP session ID.** Clients treat the combined key as
  opaque, so identical ACP session IDs on different servers stay distinct.
- **One operation per session at a time.** Prompt, load, and delete exclude each other. A
  conflicting request is answered busy and changes nothing.
- **Permission requests go to every client.** The first answer wins. Cancelling a turn answers its
  pending requests as cancelled, as ACP requires.
- **Terminals live in the daemon.** Shells outlive the GUI. The daemon keeps each terminal's screen
  state, so attaching restores the current screen, including running full-screen programs.
- **Daemon state has one lock, and nothing awaits or does IO under it.** Each change is applied and
  its events queued under the lock, which keeps each client's snapshot and live events in order, and
  ACP updates are applied in the order the server sent them.

## Runtime

The daemon runs on Tokio because the ACP Rust SDK is async. Terminal reads run on blocking threads.
The GUI core runs its socket connection on Tauri's Tokio runtime.
