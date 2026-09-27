# Test server changes in the daemon

## Goal

Fix OX-0014 by testing server addition, removal, and launch edits through daemon client requests. A
server change must settle pending requests, remove or reload only that server's sessions, and ignore
results from its retired ACP connection. A failed config file write must leave the running server
configuration intact.

## Related code

- `crates/ur/src/daemon/mod.rs` — `ServerControl` saves the config file, starts supervisors, and
  removes them. Its current launch path always creates a process, so daemon tests cannot replace a
  launch with an in-process fake server.
- `crates/ur/src/daemon/state.rs` — `server_exited()` settles session operations and permission
  requests; `remove_server()` then removes that server's sessions. Generation checks reject old ACP
  results. These paths need observable coverage, not a behavior change planned in advance.
- `crates/ur/src/daemon/tests.rs` — `TestDaemon` and `two_servers()` provide in-process fake
  servers, but `server::serve()` receives no `ServerControl`. Existing tests cover a stalled single
  server's first attempt, but do not exercise server changes through daemon client requests.
- `crates/ur/src/config.rs` — `Config::write()` uses the process-wide config path, which makes
  parallel daemon tests unsafe if they change `XDG_CONFIG_HOME`.

## Decisions

- Extend the daemon test setup with a `ServerControl`, a config file under its temporary directory,
  and a test-only way to supply or replace in-process fake server launches. Keep production process
  launching as it is. Give config saving a path-specific operation used by the test control so tests
  do not mutate process-wide environment variables.
- Exercise server changes through `AddServer`, `UpdateServer`, and `RemoveServer` requests and
  observe socket responses, watch events, and subscribed session events. Use the existing fake
  server scripts and saved history where they cover the case. Add focused controls to hold a load
  and a delete so their pending requests can be observed before changing a server.
- The existing paused-clock test owns the single-server initialize timeout. Add a distinct startup
  test only for the several-server guarantee: a stalled first attempt must not prevent a healthy
  server from initializing, and serving starts after the stalled attempt times out.

## Naming

- **Server** — one ACP agent process the daemon launches from the config file.
- **ACP connection** — the connection between the daemon and one server.
- **Daemon client** — a client connected to the daemon socket.
- **Session operation** — one prompt, load, or delete running for a session.
- **Pending permission request** — a server request held by the daemon until answered or cancelled.
- **Saved history** — the server's durable copy of a session.

## Test plan

- Adding a server publishes its state and permits creating a session on it; a name-only edit updates
  the state without interrupting its session.
- Removing a server during a running turn fails that turn, removes its session from watch and its
  subscriber, and leaves another server's session usable.
- Removing a server with a pending permission request cancels that request and removes the session.
- Removing a server while a delete waits for a turn, and while `session/delete` is in flight,
  answers each delete request with an error instead of leaving the daemon client waiting.
- A launch edit during a load answers waiting subscribes, reconnects, and loads the saved session
  from the new ACP connection. A late result from the old connection cannot replace the new
  transcript or recreate a removed session.
- A config file write failure for add, update, or remove returns an error and leaves the server
  list, connections, and sessions unchanged.
- With two configured servers, one stalled first attempt and one healthy first attempt, the healthy
  attempt completes independently; the daemon begins serving after the stalled attempt's timeout.

## Implementation plan

1. In `crates/ur/src/config.rs` and `crates/ur/src/daemon/mod.rs`, make the config write path
   selectable by `ServerControl` and add a small test-only launch seam. Keep the normal startup and
   public config behavior unchanged.
2. In `crates/ur-fake-server/src/lib.rs`, add focused controls for pending `session/load` and
   `session/delete` calls. In `crates/ur/src/daemon/tests.rs`, extend `TestDaemon` so daemon client
   server-change requests reach `ServerControl`, with separate fake server launches and saved
   histories. Add the tests above using existing watch and subscription helpers.
3. Update `agents/todo.md` and `agents/issues.csv` when the guarantees pass. Update `AGENTS.md` for
   any changed file responsibilities, and inspect the full test diff for guarantees gained or moved,
   as required by `agents/testing.md`.
