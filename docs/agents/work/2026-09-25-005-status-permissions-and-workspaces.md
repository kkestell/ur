# Status, permissions, and workspaces

## Plan

`docs/agents/plans/2026-09-25-005-status-permissions-and-workspaces.md`

## Summary

Workspaces are saved in the state file, and every session belongs to one.
`watch` sends the watch snapshot, then `workspace_added`, `workspace_removed`,
and `session_changed`. Sessions have a session status and an unread flag that
follows focus. Pending permission requests travel in the status, the first
answer wins, and cancellation answers every pending request with `Cancelled`.
Prompt errors become turn error entries and set `Failed`. The CLI gains
`ur workspace add|rm`, `ur ls`, `ur cancel`, `ur approve|deny`, and `ur wait`,
and `ur new` takes a workspace name. The plan's goal is met: the check in
`docs/agents/todo.md` passes with Ox.

Test guarantees: the ten tests the plan lists own new guarantees.
`permission_requests_are_rejected` is deleted with its guarantee.
`session_requests_fail_without_a_server` and the three session tests that stay
now add a workspace first; their guarantees are unchanged.

## Departures from the plan

- `ops::add_workspace` checks that the path is absolute and is a directory,
  before it locks `State`. The directory check reads the file system, and
  `State` does no IO. `State::add_workspace` checks the name.
- `State::remove_workspace` also takes the function that sends
  `session/cancel`, as `State::cancel` does. After the save succeeds, a failed
  `session/cancel` is logged and the removal goes ahead.

## Decisions

- `State.sessions` is a `Vec`, so the watch snapshot lists sessions in the
  order they were created.
- `cancel` publishes `session_changed` only when the status changes. Cancelling
  a `Working` session with no pending request sends none.
- An answer with an option the request does not offer answers
  `permission request <id> has no option <option>`.
- The turn error message is the SDK error's message and details on one line,
  such as `Internal error: the fake server failed`.
- The fake server's `ask` returns one error for a failed tool call notification
  and a failed permission request. `tool` reports both as `permission failed`.
- `ur read` sends `focus` after it prints the session snapshot.

## Automated checks

- `cargo fmt --all -- --check` — passed.
- `cargo test --workspace --all-targets --all-features` — passed. The daemon
  tests also passed 40 runs in a row.
- `cargo build --workspace --all-features` — passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` —
  passed.
- `pnpm -C app build` — passed.
- `pnpm -C app e2e` — passed.

## Manual verification

Setup:

```sh
T=$(mktemp -d); mkdir -p $T/config/ur $T/state $T/ws1 $T/ws2
touch $T/ws1/alpha.txt $T/ws2/beta.txt
printf '[server]\ncommand = "ox"\n' > $T/config/ur/config.toml
export UR_SOCKET=$T/ur.sock XDG_CONFIG_HOME=$T/config XDG_STATE_HOME=$T/state
target/debug/ur daemon &
P='Run the shell command `ls` in this directory and tell me what it prints.'
```

1. The check in `docs/agents/todo.md` with Ox.

   ```sh
   target/debug/ur workspace add one $T/ws1
   target/debug/ur workspace add two $T/ws2
   S1=$(target/debug/ur new one); S2=$(target/debug/ur new two)
   target/debug/ur prompt $S1 "$P"; target/debug/ur prompt $S2 "$P"
   target/debug/ur wait $S1 --until attention &
   target/debug/ur wait $S2 --until attention &
   # from a second terminal:
   target/debug/ur read $S1 --follow &
   target/debug/ur approve $S1
   target/debug/ur wait $S1
   target/debug/ur cancel $S2
   target/debug/ur wait $S2
   target/debug/ur ls
   ```

   Both waits exited 0 and printed `needs_permission` statuses with request IDs
   1 and 2 and Ox's `ls` tool call. After `approve`, `ur wait $S1` printed
   `{"type":"idle","last_stop":"end_turn"}`, and the transcript showed the tool
   call completed and the answer naming `alpha.txt`. A second `approve` exited 1
   with `session <id> has no pending permission request`. After `cancel`,
   `ur wait $S2` printed `last_stop` `cancelled`, and Ox reported the tool call
   failed as cancelled. `ur ls` listed `$S1` as `idle`, because `ur read
   --follow` focused it, and `$S2` as `idle  unread`. `ur read $S2` cleared
   the unread flag. The state file listed both workspaces.

2. Restart, removal during a turn, and a bad state file.

   ```sh
   pkill -f 'ur daemon'; target/debug/ur daemon &
   target/debug/ur ls
   S=$(target/debug/ur new one); target/debug/ur prompt $S "$P"
   target/debug/ur wait $S --until attention
   target/debug/ur read $S --follow & target/debug/ur wait $S &
   target/debug/ur workspace rm one
   target/debug/ur ls
   pkill -f 'ur daemon'; echo garbage > $T/state/ur/state.json
   target/debug/ur daemon
   ```

   After the restart, `ur ls` listed both workspaces. `ur workspace rm one`
   exited 0 while the permission request was pending. `ur read --follow` exited
   0, `ur wait` exited 1 with `session <id> was removed with workspace one`,
   and `ur ls` and the state file held only `two`. The daemon logged one
   dropped update for the removed session. With the bad state file, the daemon
   exited 1 with `parsing <path>/state.json`.
