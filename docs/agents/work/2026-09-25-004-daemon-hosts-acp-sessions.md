# Daemon hosts ACP sessions

## Plan

`docs/agents/plans/2026-09-25-004-daemon-hosts-acp-sessions.md`

## Summary

The daemon launches the server from the config file, keeps one ACP connection, and serves
`new_session`, `subscribe`, and `prompt`. `ur new`, `ur prompt`, and `ur read [--follow]` use them.
The fake server is a workspace crate, and the end-to-end suite's daemon launches it. The plan's goal
is met: the check in `docs/agents/todo.md` passes with Ox.

The five tests in `crates/ur/src/daemon/tests.rs` own new guarantees. No existing test gained, lost,
or moved a guarantee.

## Departures from the plan

- `Entry::Update` holds a `Box<SessionUpdate>`, because clippy's `large_enum_variant` rejects the
  unboxed variant. The JSON and the TypeScript bindings are unchanged.
- The response to `session/new` is sent from its callback before the server's next update is
  dispatched. So a daemon client can subscribe before the `available_commands_update` that follows
  it is applied, and the update then arrives as the first live entry instead of in the snapshot.
  `updates_right_after_session_new_are_kept` checks that the update is the first entry of the
  snapshot followed by live entries, not that it is in the snapshot.
- Moving the protocol version check into `acp::initialize` means `ur agent-run` no longer prints the
  initialize response before failing on an unsupported version. The error still names the version.

## Decisions

- `State::start_prompt` takes a `send` closure, called with the ACP connection under the lock after
  the busy check. The plan's order of steps stays in one `State` method, and `ops::prompt` only
  builds the request and its callback.
- `State` starts with the reason `the server has not started`. No request sees it, because the
  daemon accepts socket connections only after `initialize` finishes or fails.
- The fake server's `tool` script sends `selected <option ID>` or `permission failed: <error>`, and
  fails the tool call on an unknown outcome.
- `session_requests_fail_without_a_server` opens a real terminal, which runs the test process's
  `$SHELL`.

## Automated checks

- `cargo fmt --all -- --check` — passed.
- `cargo test --workspace --all-targets --all-features` — passed. The daemon tests also passed 40
  runs in a row.
- `cargo build --workspace --all-features` — passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — passed.
- `pnpm -C app build` — passed.
- `pnpm -C app e2e` — passed.

## Manual verification

1. The check in `docs/agents/todo.md` with Ox.

   ```sh
   T=$(mktemp -d); mkdir -p $T/config/ur $T/ws
   printf '[server]\ncommand = "ox"\n' > $T/config/ur/config.toml
   export UR_SOCKET=$T/ur.sock XDG_CONFIG_HOME=$T/config
   target/debug/ur daemon &
   S=$(target/debug/ur new $T/ws)
   target/debug/ur read $S --follow > $T/follow1 &
   target/debug/ur prompt $S 'Without running any commands or tools, write a four-stanza poem about the sea.'
   # about five seconds later, while the reply is streaming:
   target/debug/ur read $S --follow > $T/follow2 &
   ```

   `follow1` grew while the reply streamed. `follow2` started at 70 of the final 109 entries. When
   the turn ended, both files were identical: Ox's `available_commands_update`, the user prompt
   entry, a `session_info_update`, thought and message chunks, and a `usage_update`. The message
   chunks joined into the complete poem.

2. The CLI against the fake server, with `command` set to `target/debug/ur-fake-server` in the same
   kind of setup: `ur prompt $S "hello there world"` streamed `you said: hello there world` as one
   chunk per word, each but the last ending in a space; `ur prompt $S again` while `hold` ran
   printed `Error: session fake-1 is busy` and exited 1; `ur prompt nope hi` printed
   `Error: no session nope`; `ur new .` with no daemon named the socket path. `ur read $S` without
   `--follow` printed the same entries as the follower and exited.

## Follow-up work

- The SDK's public `send_request` does not mark the request ordered until `on_receiving_result` is
  called, so a response routed between the two calls would not hold the dispatch loop. The ops call
  `on_receiving_result` immediately, and this has not been observed. The SDK closes the gap only in
  its crate-private `send_ordered_request_to`.
