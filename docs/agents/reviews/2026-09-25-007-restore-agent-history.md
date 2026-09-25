# Restore agent history

## Scope and coverage

Reviewed commit `c9f672e` against its plan,
`docs/agents/plans/2026-09-25-006-restore-agent-history.md`: the supervisor in
`crates/ur/src/daemon/acp.rs`, the saved sessions, loads, generation, and server exit in
`crates/ur/src/daemon/state.rs`, the callbacks in `crates/ur/src/daemon/ops.rs`, the launch function
in `crates/ur/src/daemon/mod.rs`, `subscribe` in `crates/ur/src/daemon/server.rs`, the new
`SessionSummary` fields and `ur ls`, the fake server's `SavedHistory`, the daemon tests, and the
documentation changes. Callers outside the diff that depend on the changed behavior were read too:
`ur read` and `Client::request()`.

Lenses: correctness, concurrency, error-handling, testing, and documentation.

The checks with Ox in the work log were not repeated.

## Findings

### Low

#### Correctness

- **OX-0003 Removing a workspace during a load leaves waiting subscribes unanswered**
  (`crates/ur/src/daemon/state.rs:220`): if a workspace is removed while one of its sessions is
  loading, each `subscribe` waiting for that load is never answered. `remove_workspace()` sends
  `session_removed` to subscribers but drops the session with its `Op::Load` and its `waiting` list,
  and the load's callback then finds no session. `Client::request()` waits until the socket
  connection closes, so `ur read` on that session hangs. Answer each waiting `subscribe` when the
  session is removed, for example with an error that names the removed session.

#### Testing

- **OX-0004 Last activity is never tested** (`crates/ur-fake-server/src/lib.rs:190`): no test covers
  `SessionSummary.updated_at`, from `session/list` or from `session_info_update`, because the fake
  server never sends `updatedAt`. A regression that drops or never clears the last activity would
  pass the tests, and the Agent GUI will sort the sidebar by it. Have the `title` script also send
  and save an `updatedAt`, and assert it where `session_titles_come_from_updates_and_the_list`
  asserts the title.

## Checks run

- `cargo test -p ur --all-features daemon::tests`, five runs — 23 passed each time.
- `make check-docs` — passed.

## Verdict

The supervisor, loads, and server exit do what the plan describes, and the tests cover the
guarantees it lists. Two low severity findings remain open; nothing blocks the Agent GUI work.
