# Status, permissions, and workspaces

## Scope and coverage

Reviewed the implementation of
`docs/agents/plans/2026-09-25-005-status-permissions-and-workspaces.md` in
commit `861de01`: the daemon's `state.rs`, `ops.rs`, `server.rs`, `acp.rs`,
`mod.rs`, and `state_file.rs`; the new wire protocol types in `protocol.rs`;
the CLI subcommands in `crates/ur/src/cli/` and `main.rs`; the fake server's
new scripts; `gui_state.rs`; the daemon tests; and the matching sections of
`architecture.md` and `glossary.md`. The review traced each status transition,
focus and unread, first answer wins, cancellation, and workspace removal
against the plan and the Session status table in `architecture.md`.

Lenses: correctness, concurrency, error-handling, testing, simplicity,
api-design, documentation, and rust-idioms.

The TypeScript bindings were checked only as generated output. The end-to-end
suite was not run, since this change adds no GUI behavior.

## Findings

### Medium

#### Correctness

- **OX-0001 Prompting a session leaves its unread flag set**
  (`crates/ur/src/daemon/state.rs:253`): After a turn ends while no daemon
  client focuses the session, the session stays unread through the next
  prompt, because `start_prompt` does not touch `unread`. `ur wait --until
  attention` treats unread as needing attention, so after `ur prompt` it
  returns at once and prints `{"type":"working"}` instead of waiting for the
  turn to need the user. This is the normal CLI flow: prompt, wait, approve,
  wait, prompt again. Reproduced with the fake server: `ur prompt $S hi`,
  `ur wait $S`, `ur prompt $S hold`, then `ur wait $S --until attention`
  printed `{"type":"working"}` and exited 0, and `ur ls` showed
  `fake-1  working  unread`. The only way to avoid it from the CLI is an extra
  `ur read` before every prompt. Fix: clear `unread` in `start_prompt`, which
  already publishes the summary, since whoever sends a prompt has seen the
  session. Update Session status in `architecture.md` and Unread in
  `glossary.md` to say that sending a prompt also clears the flag, and extend
  `unread_follows_focus` with a prompt that clears it.

### Low

#### Testing

- **OX-0002 Cancellation during workspace removal is untested**
  (`crates/ur/src/daemon/state.rs:142`): `remove_workspace` has its own copy of
  cancellation (send `session/cancel`, then answer every pending permission
  request with `Cancelled`), separate from `State::cancel`. The plan assigns
  this guarantee to `cancel_answers_every_pending_request`, but that test only
  exercises `State::cancel`. Deleting the whole `if session.op { ... }` block in
  `remove_workspace` leaves all 15 daemon tests passing. If it regressed, the
  server would be left waiting on permission answers for a turn ur has already
  discarded. Fix: move the shared steps into one `Session` method that both
  `cancel` and `remove_workspace` call, so the existing cancellation test
  covers both paths.

## Checks run

- `cargo build --workspace --all-features` — passed.
- `cargo test --workspace --all-targets --all-features` — passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` —
  passed.
- Manual reproduction of OX-0001 with `ur daemon` and `ur-fake-server` in a
  temporary `UR_SOCKET`, `XDG_CONFIG_HOME`, and `XDG_STATE_HOME`: confirmed.
- Temporarily removed the cancellation block from `State::remove_workspace` and
  ran `cargo test -p ur daemon`: all 15 tests passed, confirming OX-0002. The
  change was reverted.

## Verdict

The implementation matches the plan and the Session status table, and the
state, ordering, and first-answer-wins logic hold up. One medium issue needs a
fix: prompting should clear the unread flag so `ur wait --until attention`
waits for the new turn. The removal cancellation test gap is low priority.
