# Restore agent history

## Plan

`docs/agents/plans/2026-09-25-006-restore-agent-history.md`

## Summary

The daemon lists each workspace's saved sessions with `session/list` after each `initialize` and
when a workspace is added, and shows their session titles and last activity in `watch` and `ur ls`.
Subscribing to or prompting an unloaded session loads it with `session/load`. The supervisor starts
the server again after it exits, fails running turns with one turn error entry, and reloads
subscribed sessions, keeping a trailing turn error. The fake server keeps a `SavedHistory` shared
between fake servers. The plan's goal is met: the check in `docs/agents/todo.md` passes with Ox.

Test guarantees: the nine new tests the plan lists own the new guarantees.
`session_requests_fail_without_a_server` now passes a launch function, and its guarantee is
unchanged. The other tests changed only their setup.

## Departures from the plan

- Each test workspace has its own directory under one temporary directory. Before, every test
  workspace used the crate directory, so `session/list` for one workspace would have returned every
  workspace's sessions.
- `State::prompt()` decides between loading first and `start_prompt()`. The load callback calls
  `start_prompt()` directly.

## Decisions

- The functions that send `session/prompt` and `session/load` take the ACP connection, the
  generation, and the request, which `State` builds with the session's workspace path.
- `server_exited()` fails every session that is `Working` or `NeedsPermission`. That includes a
  prompt still waiting for its load, which gets a turn error entry after the session snapshot of its
  unchanged transcript.
- The restart delay resets once the ACP connection is stored in `State`, after `initialize` and the
  lists.
- A failed load does not set the unread flag.
- If sending the prompt after a successful load fails, the error is logged and the session stays
  `Working` until the server exits.
- The fake server's `session/list` answers invalid params for a cursor that is not a number.

## Automated checks

- `cargo fmt --all -- --check` — passed.
- `cargo test --workspace --all-targets --all-features` — passed, and the daemon tests passed three
  runs in a row.
- `cargo build --workspace --all-features` — passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — passed.
- `pnpm -C app build` — passed.
- `pnpm -C app e2e` — passed.

## Manual verification

Setup:

```sh
T=$(mktemp -d); mkdir -p $T/config/ur $T/state $T/ws
printf '[server]\ncommand = "ox"\nargs = []\n' > $T/config/ur/config.toml
export UR_SOCKET=$T/ur.sock XDG_CONFIG_HOME=$T/config XDG_STATE_HOME=$T/state
target/debug/ur daemon &
target/debug/ur workspace add ws $T/ws; S=$(target/debug/ur new ws)
target/debug/ur prompt $S "Write a 500-word story about a lighthouse keeper. No tools."
target/debug/ur wait $S
```

1. Killing the daemon during a turn.

   ```sh
   target/debug/ur prompt $S "Now write a 1500-word sequel. No tools."
   sleep 2; pkill -f 'ur daemon'; target/debug/ur daemon &
   target/debug/ur ls; target/debug/ur read $S
   target/debug/ur prompt $S "Reply with just the word ready."
   target/debug/ur wait $S
   ```

   `ur ls` listed the session as `idle` with Ox's session title. `ur read` showed the replay: the
   first turn, then the sequel's user message without its partial reply, which Ox had not committed.
   The new prompt ended `end_turn` with the reply `ready`.

2. Killing the server during a turn.

   ```sh
   target/debug/ur read $S --follow &
   target/debug/ur prompt $S "Write a 2000-word essay on glaciers. No tools."
   sleep 3; kill -9 $(pgrep -P $(pgrep -f 'ur daemon') ox)
   target/debug/ur ls; target/debug/ur read $S
   target/debug/ur prompt $S "Reply with just the word back."
   target/debug/ur wait $S
   ```

   `ur ls` went from `working` to `failed`. The daemon logged
   `the ACP connection to ox failed: Internal error: Process exited with
   signal: 9 (SIGKILL)`, so
   a killed Ox takes the failed-connection path, not the clean close. After the restart, the
   reloaded transcript held one turn error entry with that message after the replay, followed by the
   `available_commands_update` Ox sends after answering `session/load`. The new prompt ended
   `end_turn` with the reply `back`.

## Follow-up work

- The daemon has no `unsubscribe` yet, and `reload_subscribed()` counts subscribers whose socket
  connection closed, so a server restart can reload sessions nobody is viewing.
