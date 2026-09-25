# Restore agent history

## Goal

Build the Restore agent history section of `docs/agents/todo.md`. Today the
daemon connects to the server once, and every session lives only as long as that
ACP connection. When this is done:

- At startup, the daemon lists each workspace's saved sessions with
  `session/list`, following its pagination, and shows them in `watch` with their
  session titles and last activity.
- A saved session is loaded with `session/load` when a daemon client subscribes
  to it or prompts it. Replay replaces its transcript, and subscribers get a
  fresh session snapshot.
- When the server exits, every running turn fails with one turn error entry, its
  pending permission requests are dropped, and the supervisor starts the server
  again. Subscribed sessions are reloaded, keeping the turn error at the end of
  the transcript. Other sessions load when they are next subscribed or prompted.
- A server that advertises neither capability works as today. After it
  restarts, sessions from the earlier ACP connection keep their in-memory
  transcripts but cannot be prompted.

When this is done, the check in `docs/agents/todo.md` passes with Ox.

## Related code

- `crates/ur/src/daemon/acp.rs` — `connect()` makes one ACP connection and
  records why it ended. It becomes the supervisor loop.
- `crates/ur/src/daemon/mod.rs` — `start()` builds one `AcpAgent`, and `run()`
  takes one `ConnectTo<Client>`. A restart needs a new one each time.
- `crates/ur/src/daemon/state.rs` — `Session.op` is a `bool`. `apply_update()`
  appends to the transcript. `start_prompt()` shows the pattern the load follows:
  send under the lock, then take the operation guard. `finish_prompt()` holds
  the turn-ending code that a server exit reuses.
- `crates/ur/src/daemon/ops.rs` — `new_session()` answers its request later
  from an `on_receiving_result` callback. A subscribe that starts a load does
  the same.
- `crates/ur/src/daemon/server.rs` — `handle()` returns `None` when an op
  answers later.
- `crates/ur-client/src/protocol.rs` — `SessionSummary`.
- `crates/ur-fake-server/src/lib.rs` — the fake server. It advertises no
  optional capabilities and keeps nothing between ACP connections.
- `crates/ur/src/daemon/tests.rs` — `TestDaemon::restart()` already runs a
  second daemon on the same state file.
- `crates/ur/src/cli/read.rs` — expects the session snapshot before the
  `subscribe` response. That still holds when the subscribe waits for a load.
- `agent-client-protocol-2.1.0/src/jsonrpc.rs` — `is_incoming_transport_closed()`
  identifies the error that pending requests get when the ACP connection closes
  cleanly. Their callbacks can run before or after the supervisor sees the
  close. If the connection fails with an error instead, the callbacks may never
  run.
- `~/projects/ox/src/acp.rs` — Ox advertises `loadSession` and `session/list`,
  filters `session/list` by `cwd`, and rejects a load whose `cwd` differs from
  the session's.

## Decisions

### The supervisor restarts the server with backoff

`run()` takes a launch function, `impl Fn() -> impl ConnectTo<Client>`, instead
of one server. `start()` builds a new `AcpAgent` from the config file each time
it is called. A config error still means there is no server and no restart.

`acp::supervise()` loops. Each time through, it:

- calls `connect_with`
- sends `initialize` and lists every workspace, with the 30-second timeout
  covering both
- stores the ACP connection and its capabilities in `State`, adds the listed
  saved sessions, reloads subscribed sessions, and signals ready the first time
- awaits `incoming_closed()`

When the ACP connection ends, or `initialize` fails, the supervisor calls
`State::server_exited(reason)`, waits, and starts the server again. The wait
starts at `RESTART_DELAY` (1 second), doubles up to `MAX_RESTART_DELAY` (30
seconds), and resets after a successful `initialize`. The daemon still accepts
socket connections only after the first attempt. During a restart, requests
that need the server answer `no ACP connection: <reason>`, as they do today.

`initialize` and `session/list` are awaited with `block_task()` in the
supervisor, which is not a handler. Update the Runtime section of
`docs/agents/architecture.md` to say so.

### Server exit

`State::server_exited(reason)` sets `server` to `Err(reason)` and marks every
session unloaded, since the new server process has loaded none of them. For
each session whose operation guard is held:

- If a turn is running (`Working` or `NeedsPermission`), it appends
  `Entry::TurnError { message: reason }` and sets `Failed { message: reason }`.
  It drops the responders, clears `cancelling`, sets unread when nothing focuses
  the session, and publishes. `finish_prompt()` and this share one `Session`
  method, `end_turn(result)`.
- If a load is running, it discards the replay and sends the session snapshot
  of the unchanged transcript to subscribers, as a failed load does.
- It releases the operation guard.

The reason is the one `connect()` builds today, such as `the fake server closed
the ACP connection`.

`State` gains `generation: u64`, incremented by each `set_server(Ok)`. Prompt
and load callbacks carry the generation they were sent in. `finish_prompt()` and
`finish_load()` ignore a result from an earlier generation, so a late result
cannot release an operation guard that a request on the new ACP connection now
holds. A callback also ignores an error for which `is_incoming_transport_closed()`
is true. The supervisor records that exit, so every interrupted turn gets the
same single turn error.

### Saved sessions come from `session/list`

When the server advertises `sessionCapabilities.list`, `acp::list_sessions(
connection, path)` calls `session/list` with `cwd` set to the workspace path and
follows `nextCursor` until it is absent. The supervisor calls it for every
workspace after each `initialize`. `ops::add_workspace()` also calls it, in a
spawned task, for the new workspace, so re-adding a workspace brings back its
saved sessions. A list error is logged, and that workspace shows only the
sessions already in `State`.

`State::add_saved_sessions(workspace, sessions)` adds each listed session that
`State` does not have as a saved session: `Idle { last_stop: None }`, not
unread, and unloaded. For a session `State` already has, it updates only the
session title and last activity. It does nothing if the workspace was removed
while the list was in flight. Sessions stay in the order they were added.

`SessionSummary` gains `title: Option<String>` and `updated_at: Option<String>`,
the last activity. They come from `session/list` and from `session_info_update`,
live or replayed. A value of `null` clears the field, and an absent field leaves
it unchanged. A change publishes `session_changed`. `ur ls` prints the session
title after the status when there is one.

### Loads

`Session` gains `loaded: bool`, which is true for sessions created by
`session/new` during the current ACP connection. `op: bool` becomes
`op: Option<Op>`:

- `Op::Prompt` — a turn is running.
- `Op::Load { replay, waiting }` — `session/load` is in flight. `replay` holds
  the replayed entries. `waiting` holds the request ID and outbox of each
  `subscribe` that answers when the load ends.

While a load runs, `apply_update()` appends to `replay` instead of the
transcript and sends nothing to subscribers. `session/load` passes the workspace
path as `cwd`.

A load starts when:

- A daemon client subscribes to an unloaded session that is not `Failed`, while
  the server advertises `loadSession` and the operation guard is free. The
  daemon registers the subscriber, adds its request to `waiting`, and sends no
  snapshot yet. A subscribe that arrives during any load waits the same way.
  Otherwise, `subscribe` answers at once with the in-memory transcript, as it
  does today. That covers a `Failed` session: the transcript it has is the one
  that failed.
- A daemon client prompts an unloaded session. Without `loadSession`, the prompt
  answers `the server cannot load session <id>`. Otherwise, the daemon takes
  the operation guard with `Op::Load`, sets `Working`, clears unread, and
  answers `done` once `session/load` is sent. `Working` from the start means
  `ur wait` after `ur prompt` does not see the old `Idle`. The prompt's content
  stays in the load callback, which sends the prompt when the load succeeds. A
  cancel during this load does nothing.
- The server restarts. After listing, the supervisor calls
  `State::reload_subscribed()`, which loads every unloaded session that has
  subscribers, including `Failed` ones.

`State::finish_load(id, generation, result)`:

- On success, the transcript becomes `replay`. If the old transcript ended with
  a turn error entry, that entry is appended after the replay. This keeps the
  error for a turn the server exit interrupted. The session is loaded, and its
  status is unchanged.
- On failure, the transcript is unchanged, the session stays unloaded, and the
  status becomes `Failed { message: "session/load failed: <error>" }`. No
  transcript entry is added. The next prompt tries the load again.
- In both cases, it releases the operation guard, sends a session snapshot to
  every subscriber, and answers each waiting `subscribe` with `done`. The
  status tells a daemon client that the load failed.

For a prompt, the load callback calls `finish_load()`, then `start_prompt()`
under the same lock, so no other request can take the operation guard in
between.

### The fake server keeps saved history

`fake_server(hold, history)` takes a `SavedHistory`, a cloneable handle that
the tests share between fake server instances, so saved sessions outlive one
ACP connection and one daemon. It holds each session's ID, `cwd`, session
title, every update the fake server sent for it, and a `user_message_chunk` for
each prompt's text. The fake server saves these chunks for replay only and
never sends them live, so live transcripts are unchanged. The session number
counter moves into `SavedHistory`, so IDs are not reused after a restart. The
binary uses `SavedHistory::default()`, which lasts as long as its process.

With `SavedHistory::default()`, the fake server advertises `loadSession` and
`sessionCapabilities.list`:

- `session/list` returns the sessions whose `cwd` matches, one per page, with
  the next index as `nextCursor`, so the daemon's pagination runs.
- `session/load` replays the saved updates as notifications, then answers. An
  unknown session answers an error.
- `session/prompt` for a session not created or loaded during this ACP
  connection answers `session <id> is not loaded`, as Ox does. A daemon that
  forgets to load after a restart fails the tests.

`SavedHistory::unadvertised()` advertises neither capability and answers
`session/list` and `session/load` with a method-not-found error.

New scripts:

- `title`: sends `session_info_update` with the session title `tallies`, which
  the fake server also saves for `session/list`, and ends the turn.
- `unloadable`: replies, then marks the session so every later `session/load`
  fails with `the fake server cannot load this session`.

## Naming

- supervisor, generation, saved history, saved session, loaded session, load,
  replay, session title, last activity, operation guard, busy, turn error entry,
  session snapshot, fake server — as defined in `docs/agents/glossary.md`.
- `acp::supervise()` — the supervisor loop that replaces `acp::connect()`.
- `RESTART_DELAY`, `MAX_RESTART_DELAY` — the supervisor's first and longest wait
  before starting the server again.
- `State::server_exited(reason)`, `add_saved_sessions()`, `reload_subscribed()`,
  `finish_load()`; `Session::end_turn()` — the `State` methods above.
- `Server { connection, capabilities }` — the ACP connection and the
  capabilities from its `initialize`, held in `State.server` as
  `Result<Server, String>`.
- `Op::Prompt`, `Op::Load { replay, waiting }` — the session operation that
  holds `session.op`.
- `loaded` — the `Session` flag for a loaded session.
- `SessionSummary.title`, `SessionSummary.updated_at` — the session title and
  last activity in `watch`.
- `acp::list_sessions()` — every page of `session/list` for one workspace path.
- `SavedHistory`, `SavedHistory::unadvertised()` — the fake server's saved
  history, with and without its history capabilities.
- `title`, `unloadable` — the new fake server scripts.
- `TestDaemon::kill_server()` — aborts the running fake server task, which
  closes its ACP connection the way a server exit does.

## Test plan

In `crates/ur/src/daemon/tests.rs`, `TestDaemon` holds a `SavedHistory`, which
`restart()` passes on, and a launch function that spawns a fake server on a new
`Channel::duplex()` and keeps its abort handle for `kill_server()`. Restarts in
these tests wait for `RESTART_DELAY`.

- `saved_sessions_are_listed_at_startup`: two sessions in `home` and one in
  `work`. After a daemon restart, the watch snapshot lists all three under their
  workspaces, `Idle { last_stop: None }` and not unread. Two sessions in one
  workspace means the daemon follows `nextCursor`.
- `subscribing_loads_a_saved_session`: a session is prompted with `hold` and
  never released, which is a daemon killed during a turn. After a daemon
  restart, `subscribe` answers after a snapshot holding the replay: the
  commands update, the user message chunk `hold`, and `holding`.
- `prompting_loads_a_saved_session_first`: after a daemon restart, a prompt to
  a saved session that nobody subscribed to ends
  `Idle { last_stop: end_turn }`. A later snapshot is the replay, the user
  prompt entry, and the reply.
- `a_failed_load_is_retried_by_the_next_prompt`: after `unloadable` and a daemon
  restart, `subscribe` answers and the session becomes `Failed` with the fake
  server's message. The session stays in `watch`. A prompt sets `Working`, then `Failed` again.
- `session_titles_come_from_updates_and_the_list`: `title` sends
  `session_changed` with the session title `tallies`. After a daemon restart,
  the watch snapshot has it too.
- `adding_a_workspace_lists_its_saved_sessions`: after a session is created in
  `home`, `home` is removed and added again, and the session returns in a
  `session_changed`.
- `server_exit_fails_the_running_turn`, table-driven over `hold` (`Working`)
  and `tool` (`NeedsPermission`): after `kill_server()`, the session is
  `Failed` with the reason and unread, its transcript ends with exactly one turn
  error entry holding that reason, and answering the old request ID answers
  `permission request <id> is resolved`.
- `subscribed_sessions_reload_after_the_server_restarts`: a subscribed session
  interrupted during `hold` gets a fresh session snapshot after the restart. It
  holds the replay followed by the turn error entry, and the status is still
  `Failed`. The next prompt ends `Idle`.
- `without_history_capabilities_old_sessions_cannot_be_prompted`: with
  `SavedHistory::unadvertised()`, after `kill_server()` and the restart, a
  prompt to the old session answers `the server cannot load session <id>`. Its
  subscriber's snapshot is still the in-memory transcript, and a new session
  can be created and prompted.
- `session_requests_fail_without_a_server`: the silent server's launch function
  keeps each agent side open. With the paused clock, restarts time out the same
  way, so the reason is unchanged.
- The other tests only change how they start the fake server.

The end-to-end suite gains no tests. Run it at the end, since this finishes a
section of `docs/agents/todo.md`. Run the check in `docs/agents/todo.md` with Ox
by hand.

## Implementation plan

The groups follow the parts of the section in `docs/agents/todo.md`.

Listing saved sessions:

1. `crates/ur-client/src/protocol.rs`: `SessionSummary.title` and
   `updated_at`. `crates/ur/src/cli/ls.rs`: print the session title.
2. `crates/ur-fake-server/src/lib.rs`: `SavedHistory`, the capabilities,
   `session/list` with one session per page, and the `title` script.
   `main.rs`: pass `SavedHistory::default()`.
3. `crates/ur/src/daemon/state.rs`: `Server`, `add_saved_sessions()`, `loaded`,
   and the session title and last activity from `session_info_update` in
   `apply_update()`. `crates/ur/src/daemon/acp.rs`: `list_sessions()`, called
   after `initialize` inside the timeout. `ops.rs`: `add_workspace()` lists the
   new workspace.

Loading:

4. `crates/ur-fake-server/src/lib.rs`: `session/load`, saved user message
   chunks, rejecting prompts for sessions that are not loaded, and the
   `unloadable` script.
5. `state.rs`: `Op`, the replay in `apply_update()`, `finish_load()`, and
   subscribe and prompt starting a load. `ops.rs`: `load()` builds the
   `session/load` callback, with the prompt's content when there is one.
   `ops::subscribe()` answers later when it starts a load. `server.rs`: handle
   `Subscribe` through it.

Server exit and restart:

6. `state.rs`: `generation`, `Session::end_turn()`, `server_exited()`, and
   `reload_subscribed()`. `ops.rs`: callbacks carry the generation and ignore
   closed-transport errors.
7. `acp.rs`: `supervise()` with `RESTART_DELAY` and `MAX_RESTART_DELAY`.
   `mod.rs`: `start()` passes a launch function, and `run()` takes one.

Tests:

8. `crates/ur/src/daemon/tests.rs`: the helpers and tests in the Test plan.
   Regenerate `app/src/ipc/bindings/` with `cargo test`.
9. Run the end-to-end suite and the check in `docs/agents/todo.md` with Ox.

## Documentation updates

- `AGENTS.md`: `acp.rs` becomes `supervise()`, `list_sessions()`, and
  `initialize()`. `ops.rs` gains `load()` and `subscribe()`. `state.rs` gains
  saved sessions, loads, and the generation. `ur-fake-server` exports
  `SavedHistory`.
- `docs/agents/architecture.md`: under One ACP server process per daemon, give
  the restart delays. Under The ACP server owns saved history, say when a load
  starts, that a `subscribe` waiting for a load answers when it ends, that a
  trailing turn error entry survives a reload, and that `session/list` runs
  after each `initialize` and when a workspace is added. Say that without
  `loadSession`, sessions from an earlier ACP connection cannot be prompted.
  Under Daemon architecture, describe `Op`, the replay held in it, and the
  closed-transport rule for callbacks. Under Runtime, say that `session/list` is
  awaited with `block_task()` in the supervisor, like `initialize`.
- `docs/agents/glossary.md`: add the session title and last activity to Session
  summary, and `SessionSummary.updated_at` to the last activity row of Names
  across boundaries. Add `title` and `unloadable` to Fake server, and say that
  its saved history is a `SavedHistory`. Say under Load that subscribing,
  prompting, and a server restart start it.
