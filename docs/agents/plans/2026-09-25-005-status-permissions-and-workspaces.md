# Status, permissions, and workspaces

## Goal

Build the Status, permissions, and workspaces section of `docs/agents/todo.md`.
Workspaces are saved in the state file, and every session belongs to one. A
daemon client that sends `watch` gets a watch snapshot of workspaces and
sessions, then every change to them. Each session has a session status and an
unread flag. Pending permission requests reach every watching daemon client,
the first answer wins, and cancellation answers every pending request with
`Cancelled`. Turn errors become transcript entries and set `Failed`. The CLI
gains `ur workspace add|rm`, `ur ls`, `ur cancel`, `ur approve|deny`, and
`ur wait`, and `ur new` takes a workspace name.

When this is done, the check in `docs/agents/todo.md` passes with Ox: two sessions in
two workspaces, `ur wait --until attention` on both, and one approved from a
second terminal.

## Related code

- `crates/ur/src/daemon/state.rs` — `State` and `Session`. Gains workspaces,
  watchers, status, unread, focus, and pending permission requests.
  `start_prompt(send)` shows the pattern the workspace saves follow: do the
  step that can fail first, under the lock, so a failure leaves nothing to undo.
- `crates/ur/src/daemon/acp.rs` — the permission handler, which today rejects
  every request.
- `crates/ur/src/daemon/ops.rs` — `new_session` and `prompt`. The prompt
  callback today only logs an error and clears `op`.
- `crates/ur/src/daemon/server.rs` — `handle()` and `connection()`. The end of
  `connection()` is where a daemon client's focus clears.
- `crates/ur/src/daemon/mod.rs` — `start()` and `run()`. They gain the state
  file.
- `crates/ur/src/daemon/tests.rs` — the daemon tests, with helpers `TestDaemon`,
  `Subscription`, `new_session`, `subscribe`, and `prompt`.
- `crates/ur-client/src/protocol.rs` — the wire protocol types.
- `crates/ur-fake-server/src/lib.rs` — the fake server's scripts. `tool()`
  holds the permission request code that the new scripts share.
- `crates/ur/src/cli/` and `crates/ur/src/main.rs` — the CLI.
- `app/src-tauri/src/gui_state.rs` — `path()` works out the state directory. The
  daemon needs the same directory.
- `crates/ur/tests/terminal.rs` — runs `ur daemon` with `HOME` set to a
  temporary directory. The daemon now reads the state file there.
- `agent-client-protocol-2.1.0/src/jsonrpc.rs` — dropping a `Responder` sends
  no reply, so clearing a pending request at the end of a turn only drops it.

## Decisions

### Watch sends a snapshot, then change events

`watch` queues `watch_snapshot` and adds the outbox to `State.watchers` under
one lock, the same way `subscribe` does. Watching again from the same socket
connection replaces the earlier registration. After that, the daemon sends:

- `workspace_added { workspace }`
- `workspace_removed { name }`, which also removes the workspace's sessions
- `session_changed { summary }`, sent when a session is added or its status or
  unread flag changes. It carries the session's whole `SessionSummary`.

Workspaces are listed in the order they were added, and sessions in the order
they were created. Session titles and last activity are left for the Restore
agent history section of `docs/agents/todo.md`.

### Pending permission requests travel in the session status

`Status::NeedsPermission { requests }` carries each `PendingPermission`, oldest
first, as the status table in `docs/agents/architecture.md` already specifies. Watch
therefore delivers every pending permission request to every watching daemon
client, and a status change tells them a request is resolved. The session
snapshot stays as it is: the transcript only. `docs/agents/architecture.md` and
`docs/agents/glossary.md` are updated to match.

The daemon numbers pending permission requests from 1 with a `u32` counter
shared by all sessions. It does not use the JSON-RPC ID, which can be a string,
a number, or null. `u32` keeps the TypeScript binding `number`, as for
`TerminalId`.

### Session status

`Session` gains `status: Status`, `unread: bool`, `focus: Vec<Outbox>`,
`responders: HashMap<u32, Responder<RequestPermissionResponse>>` (keyed by
request ID, alongside the requests listed in the status), and `cancelling:
bool`. Status changes follow the table under Session status in
`docs/agents/architecture.md`:

- A new session is `Idle { last_stop: None }`.
- Sending a prompt sets `Working`. This also clears `Failed`.
- A permission request sets `NeedsPermission` and appends to `requests`. While
  `cancelling` is set, the handler answers `Cancelled` at once and the status
  stays `Working`. A request for an unknown session gets an internal error.
- Answering the last pending request sets `Working`.
- The prompt callback sets `Idle { last_stop }` for a stop reason. For a
  JSON-RPC error, it appends `Entry::TurnError { message }` and sets
  `Failed { message }`. In both cases it drops any remaining responders, clears
  `op` and `cancelling`, and sets `unread` when `focus` is empty.

A rejected prompt takes the same path. Its turn error entry follows its user
prompt entry because no update arrives in between.

Server exit during a turn still leaves the session `Working`. The Restore agent
history section of `docs/agents/todo.md` handles that.

### Focus

`focus { sessions }` replaces the socket connection's focus: the daemon removes
its outbox from every session's `focus` and adds it to each listed session's.
Focusing a session clears its unread flag. An unknown session answers an error
and changes nothing. `State::disconnect(outbox)`, called when `connection()` in
`server.rs` ends, removes the outbox from every session's focus.

`ur read` focuses its session after subscribing, because a daemon client
focuses the sessions it shows. Reading a session clears its unread flag, and a
session followed with `--follow` does not become unread. Without this, the CLI
could never clear unread, and `ur wait --until attention` would return at once
for any session that had finished a turn.

### Answering and cancellation

`answer_permission { session, request_id, option_id }` responds through the
stored responder with `Selected(option_id)`, removes the request, and publishes
the session's new status. A request that is not pending answers an error,
`permission request <id> is resolved`. First answer wins because the first
answer removes the request. An option ID that is not in the request answers an
error and changes nothing.

`cancel { session }` on a session with no running prompt answers `done` and
does nothing. Otherwise, under the lock, it sends `session/cancel`, answers
every pending request with `Cancelled`, sets `cancelling`, and sets `Working`.
The operation guard stays held until the prompt returns.

### Workspaces and the state file

`daemon/state_file.rs` reads and writes `state.json` in `ur_client::state_dir()`
as `{"workspaces": [{"name": ..., "path": ...}]}`. `state_dir()` moves the
directory logic out of `app/src-tauri/src/gui_state.rs` into `ur-client`, next
to `socket_path()`, so the daemon and the core agree on the directory. A missing
file means no workspaces. The daemon reads the file at startup, and an
unreadable file stops the daemon with an error that names the path. Reading at
startup belongs here, since without it the file has no reader. The Restore
agent history section of `docs/agents/todo.md` adds `session/list` on top.

`add_workspace { name, path }` answers an error for an empty name, a name
already in use, or a path that is not an absolute path to a directory. The
CLI makes the path absolute with `std::path::absolute`, as `ur new` does today.
The daemon stores the path as is.

`State::add_workspace(workspace, save)` and `State::remove_workspace(name,
save)` check the request, call `save` with the new list, and change `State` only
if `save` succeeds. The state file is written under the lock, so saves happen
in order, and `State` itself still does no IO.

`remove_workspace { name }`, under the lock, saves the new list. Then it cancels
the workspace's running prompts as `cancel` does, sends `session_removed` to
each of its sessions' subscribers, removes those sessions and the workspace, and
publishes `workspace_removed`. It does not wait for the cancelled prompts to
return. Their callbacks find no session and do nothing. Their later updates are
dropped as updates for an unknown session. If the workspace is removed while
`session/new` is in flight, the `session/new` callback answers an error instead
of adding the session.

`new_session { workspace }` replaces `new_session { path }`. It passes the
workspace path as `cwd` and records the workspace name on the session.

### CLI output

- `ur workspace add <name> <path>` and `ur workspace rm <name>` print nothing.
- `ur new <workspace>` prints the session ID.
- `ur ls` prints each workspace as `<name>  <path>`. Under it, each session is
  printed as `  <session>  <status>`, where status is `idle`, `working`,
  `needs permission`, or `failed`, followed by `  unread` when the unread flag
  is set.
- `ur cancel <session>` prints nothing.
- `ur approve <session>` and `ur deny <session>` read the session's status from
  the watch snapshot and answer its oldest pending request with the first
  `allow_*` or `reject_*` option. A session with no pending request, or a
  request with no such option, exits non-zero with a message.
- `ur wait <session> [--until attention|idle]` watches until the condition
  holds, prints the session's status as one JSON line, and exits. `idle`, the
  default, holds when no turn is running: `Idle` or `Failed`. `attention` holds
  when the session needs attention. An unknown session, or one whose workspace
  is removed, exits non-zero.
- `ur read --follow` also exits on `session_removed`.

### Fake server scripts

`tool()`'s permission request becomes `ask(tool_call_id)`, which sends the tool
call, asks for permission, and returns the outcome. New scripts:

- `tools`: asks for `tally-1` and `tally-2` at once. After both are answered, if
  either was cancelled, it asks for `tally-3`. A well-behaved server would not
  ask again after a cancellation, but this lets tests check that the daemon
  answers it `Cancelled`. It then sends one message listing each outcome, such
  as `tally-1: go, tally-2: cancelled`, and returns `cancelled` if any request
  was cancelled, else `end_turn`.
- `reject`: answers the prompt with the error `the fake server rejects this
  prompt` before sending any update.
- `fail`: sends `failing`, then answers with the error `the fake server failed`.

## Naming

- workspace, workspace path, state file, watch, subscribe, session status,
  unread, needs attention, focus, pending permission request, permission
  option, first answer wins, resolved, cancellation, turn error entry,
  rejected prompt, busy, operation guard, outbox, `State`, fake server — as
  defined in `docs/agents/glossary.md`.
- `Workspace { name, path }` — a workspace on the wire and in the state file.
- `SessionSummary { session, workspace, status, unread }` — one session as watch
  shows it. Added to `docs/agents/glossary.md` as session summary.
- `PendingPermission { request_id, request }` — a pending permission request
  on the wire: the daemon's request ID and the ACP `RequestPermissionRequest`.
- request ID — the daemon's `u32` number for a pending permission request.
  Added to the glossary entry for pending permission request.
- `Status` — `Idle { last_stop: Option<StopReason> }`, `Working`,
  `NeedsPermission { requests: Vec<PendingPermission> }`, and
  `Failed { message }`.
- `Entry::TurnError { message }` — a turn error entry.
- `Request::Watch`, `AddWorkspace`, `RemoveWorkspace`, `Focus`, `Cancel`,
  `AnswerPermission`; `Event::WatchSnapshot { workspaces, sessions }`,
  `WorkspaceAdded`, `WorkspaceRemoved`, `SessionChanged`, `SessionRemoved` —
  the wire protocol names above.
- `cancelling` — the `Session` flag set by cancellation until the prompt
  returns.
- `tools`, `reject`, `fail` — the new fake server scripts.

## Test plan

Tests in `crates/ur/src/daemon/tests.rs`. `TestDaemon::run` gives each daemon a
state file in its temporary directory. The `new_session` helper adds a
workspace first. A new `Watch` helper, like `Subscription`, waits with a timeout
for the next `session_changed` for a session and returns its summary.

- `watch_sends_the_snapshot_then_changes`: a watcher that starts before any
  workspace exists receives `workspace_added`, then `session_changed` with
  `Idle { last_stop: None }` for a new session. A second watcher's snapshot
  holds the same workspace and session.
- `workspaces_survive_a_daemon_restart`: after two workspaces are added and one
  is removed, a second daemon on the same state file lists the remaining one in
  its watch snapshot.
- `workspace_requests_reject_bad_input`, table-driven: adding with a relative
  path, a path that is not a directory, an empty name, or a name in use;
  creating a session in an unknown workspace; and removing an unknown workspace.
  Each answers an error, and a new watcher's snapshot is unchanged.
- `removing_a_workspace_removes_its_sessions`: while a session in the workspace
  runs `tool` with a pending request, removal sends watchers
  `workspace_removed`, sends the session's subscriber `session_removed`, and
  later requests for the session answer an error. A session in another
  workspace is untouched. The cancellation that removal performs is owned by
  `cancel_answers_every_pending_request`.
- `status_follows_the_turn`: prompting `tool` moves the session through
  `Working`, `NeedsPermission` with one request holding the options `go` and
  `stop`, `Working` after answering `go`, and `Idle { last_stop: end_turn }`.
  The transcript ends with the message `selected go`.
- `first_answer_wins`: two daemon clients answer the same request, one with
  `go` and one with `stop`. The first gets `done` and the second an error, and
  the transcript's message names the first answer's option.
- `several_pending_requests_are_kept_oldest_first`: `tools` gives
  `NeedsPermission` with `tally-1` then `tally-2`. Answering `tally-2` leaves
  only `tally-1`. Answering it gives `Working`, then `Idle`, with the message
  `tally-1: go, tally-2: stop`.
- `cancel_answers_every_pending_request`: with both `tools` requests pending,
  `cancel` sets `Working` with no requests. The status never returns to
  `NeedsPermission` for `tally-3`, and the turn ends
  `Idle { last_stop: cancelled }` with every outcome `cancelled` in the
  message.
- `unread_follows_focus`, table-driven over who focuses the session when the
  turn ends: a daemon client that still focuses it (stays read); nobody (turns
  unread, and a later `focus` clears it); a daemon client that focused it and
  then disconnected (turns unread). The disconnect case drops its client while
  a `hold` turn runs and releases the hold afterwards.
- `prompt_errors_fail_the_turn`, table-driven over `reject` and `fail`: the
  transcript ends with a turn error entry holding the fake server's message.
  For `reject` it comes right after the user prompt entry, and for `fail` after
  `failing`. The status is `Failed` with that message, and the next prompt sets
  `Working`.
- `overlapping_prompt_is_busy` stays as it is. It owns the busy second prompt
  that leaves the first running.
- `permission_requests_are_rejected` is deleted. Its guarantee is gone.
- `session_requests_fail_without_a_server` adds a workspace first, which works
  without a server.

`crates/ur/tests/terminal.rs` sets `XDG_STATE_HOME` to its temporary directory.
The end-to-end suite gains no tests. Run it at the end, since this finishes a
section of `docs/agents/todo.md` and `gui_state.rs` changes. Run the check in
`docs/agents/todo.md` with Ox by hand.

## Implementation plan

The groups follow the numbered tasks in the section of `docs/agents/todo.md`.

Workspaces and watch:

1. `crates/ur-client/src/protocol.rs`: `state_dir()`, `Workspace`, `Status` (at
   first only `Idle` and `Working`), `SessionSummary`, `Request::Watch`,
   `AddWorkspace`, `RemoveWorkspace`, `NewSession { workspace }`,
   `Event::WatchSnapshot`, `WorkspaceAdded`, `WorkspaceRemoved`,
   `SessionChanged`, and `SessionRemoved`, with ts-rs overrides for the ACP
   types. `app/src-tauri/src/gui_state.rs`: use `state_dir()`.
2. `crates/ur/src/daemon/state_file.rs`: `read` and `write`.
   `crates/ur/src/daemon/mod.rs`: `run` takes the state file path, reads it into
   `State`, and passes the path to `server::serve`.
3. `crates/ur/src/daemon/state.rs`: workspaces, watchers, `watch`,
   `add_workspace`, `remove_workspace`, `add_session(id, workspace)`, and a
   `publish(session)` helper that queues `session_changed` to watchers.
   `ops.rs`: `new_session` looks up the workspace. `add_workspace` and
   `remove_workspace` pass `save` closures that write the state file.
   `server.rs`: handle the new requests.
4. `crates/ur/src/cli/workspace.rs` and `ls.rs`; `cli/mod.rs` gains a
   `watch(client)` helper that returns the snapshot and the event receiver;
   `new.rs` takes a workspace name. `main.rs`: `Workspace { Add | Rm }`, `Ls`,
   and `New { workspace }`.

Status, focus, and unread:

5. `protocol.rs`: `Request::Focus`. `state.rs`: `status`, `unread`, and
   `focus` on `Session`; `focus(outbox, sessions)` and `disconnect(outbox)`;
   `start_prompt` sets `Working` and publishes; `finish_prompt(id,
   Result<StopReason, String>)` sets `Idle`, sets unread, and publishes.
   `server.rs`: call `disconnect` when `connection()` ends. `cli/read.rs`:
   send `focus` after subscribing.

Permissions and cancellation:

6. `protocol.rs`: `PendingPermission`, `Status::NeedsPermission`,
   `Request::Cancel`, and `Request::AnswerPermission`. `state.rs`: responders,
   `cancelling`, `request_permission`, `answer_permission`, and `cancel(send)`.
   `remove_workspace` cancels running prompts. `acp.rs`: the permission handler
   calls `request_permission`. `ops.rs`: `cancel` sends `session/cancel` with a
   `CancelNotification`.
7. `crates/ur/src/cli/cancel.rs` and `answer.rs` (`approve` and `deny`).
   `main.rs`: `Cancel`, `Approve`, and `Deny`.

Turn errors:

8. `protocol.rs`: `Entry::TurnError` and `Status::Failed`. `ops.rs`: the prompt
   callback passes the error message to `finish_prompt`, which appends the turn
   error entry and sets `Failed`.

Wait and tests:

9. `crates/ur/src/cli/wait.rs`. `main.rs`: `Wait { session, until }`, with
   `until` a clap `ValueEnum` of `idle` and `attention`, defaulting to `idle`.
10. `crates/ur-fake-server/src/lib.rs`: `ask`, and the `tools`, `reject`, and
    `fail` scripts.
11. `crates/ur/src/daemon/tests.rs`: the tests in the Test plan.
    `crates/ur/tests/terminal.rs`: set `XDG_STATE_HOME`. Regenerate
    `app/src/ipc/bindings/` with `cargo test`.
12. Run the end-to-end suite and the check in `docs/agents/todo.md` with Ox.

## Documentation updates

- `AGENTS.md`: map `daemon/state_file.rs`, `cli/workspace.rs`, `cli/ls.rs`,
  `cli/cancel.rs`, `cli/answer.rs`, and `cli/wait.rs`. Update the `main.rs`,
  `cli/`, `protocol.rs`, `state.rs`, and `mod.rs` entries.
- `docs/agents/architecture.md`: under Wire protocol, say that `subscribe` covers the
  transcript and config options, and that pending permission requests travel in
  the session status in `watch`. List the watch change events, and change
  `new_session` to take a workspace. Under Permissions, give the daemon's
  request IDs. Under Session status, say that a daemon client focuses the
  sessions it shows, including `ur read`. Under The ACP server owns saved
  history, say that the daemon reads the state file at startup and stops when
  it cannot.
- `docs/agents/glossary.md`: remove pending permission requests from Subscribe and
  Session snapshot. Add them to Watch. Add session summary, add the request ID
  to Pending permission request, and add `tools`, `reject`, and `fail` to Fake
  server.
