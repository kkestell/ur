# Daemon hosts ACP sessions

## Scope and coverage

Reviewed commit `74964bb` against
`docs/agents/plans/2026-09-25-004-daemon-hosts-acp-sessions.md` and the Daemon hosts
ACP sessions section of `docs/agents/todo.md`: `crates/ur/src/daemon/` (`mod.rs`,
`acp.rs`, `ops.rs`, `state.rs`, `server.rs`, `tests.rs`), `crates/ur/src/cli/`,
`crates/ur/src/main.rs`, the `one_shot.rs` change, `crates/ur-client`
(`protocol.rs`, `client.rs`, the generated bindings), `crates/ur-fake-server`,
`app/e2e/harness.ts`, `app/package.json`, the Cargo manifests, and the
documentation updates. Where the ordering design depends on the ACP SDK
(`on_receiving_result`, `consume_with`, `incoming_closed`, channel types), I
read its source in `agent-client-protocol-2.1.0`.

Lenses: correctness, concurrency, error-handling, testing, api-design,
documentation, rust-idioms.

Gaps: I did not run the end-to-end suite or the check in `docs/agents/todo.md` with
Ox.

## Findings

### Medium

#### Error handling

- **A server that never answers `initialize` stops the whole daemon**
  (`crates/ur/src/daemon/acp.rs:47`, `crates/ur/src/daemon/mod.rs:47`): `run`
  awaits `acp::connect`, which returns only after `initialize` finishes or
  fails. If the configured command starts but never answers, the daemon never
  accepts socket connections. Every daemon client waits forever, including
  terminals, which the plan says keep working when the server cannot start.
  Nothing is logged. Evidence: with `command = "sleep"` and
  `args = ["1000"]`, `ur new` was still waiting after 5 seconds
  (`timeout` exit 124), and the daemon's stderr was empty. A misconfigured
  command that reads stdin without speaking ACP does the same, and so does a
  slow-starting server, until it answers. Fix: put a timeout around
  `initialize` in the `connect_with` closure, hardcoded next to it, and treat
  it as an `initialize` failure. The existing path then logs the reason and
  answers session requests with it. Fixed: `acp::connect` gives `initialize`
  30 seconds, and `session_requests_fail_without_a_server` now also runs the
  daemon against a server that never answers.

### Low

#### Error handling

- **Server start errors do not name the command and include SDK internals**
  (`crates/ur/src/daemon/acp.rs:59`): the reason is built with the SDK error's
  `Display`, which pretty-prints its JSON `data`, including the SDK's source
  location. The configured command is never named. Evidence: with
  `command = "no-such-command-xyz"`, both the daemon's log and `ur new` print:

  ```text
  no ACP connection: the ACP connection failed: Internal error: {
    "spawned_at": ".../agent-client-protocol-2.1.0/src/jsonrpc.rs:1931:39",
    "data": "No such file or directory (os error 2)"
  }
  ```

  A missing command, `command = "false"`, and a server killed mid-turn all
  produce the same shape. Fix: pass the configured command into
  `acp::connect`, or add it to the reason in `run`. Format the SDK error from
  its `message`, plus the inner `data` string when there is one, so the reason
  reads like `cannot run no-such-command-xyz: No such file or directory
  (os error 2)`. Fixed: `run` passes the configured command to
  `acp::connect`, which names it in the reason, and `acp::describe` formats
  SDK errors on one line without the task locations. `ops.rs` and
  `acp::initialize` use it too. The daemon test for a server that never
  answers checks that the reason names the server.

## Checks run

- `cargo fmt --all -- --check` passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
  passed.
- `cargo test --workspace --all-targets --all-features` passed. The daemon
  tests passed 20 more runs in a row.
- Ran the daemon with a config whose command never answers (`sleep 1000`):
  `ur new` hung, and nothing was logged.
- After the fix, with the same config: `ur new` exited 1 after 30 seconds
  with `no ACP connection: initialize failed: no answer after 30 seconds`, the
  daemon logged the same reason, and the `sleep` process was gone. Without the
  fix, the extended `session_requests_fail_without_a_server` hangs.
- Ran the daemon with `command = "false"` and with a missing command: the
  daemon kept running, logged the reason, and `ur new` exited 1 with it.
- Ran the daemon with the fake server binary, prompted `hold`, and killed the
  fake server: the next `ur prompt` exited 1 with the reason, and `ur read`
  still printed the transcript held in memory.
- After the error wording fix, the same three failures report
  `the ACP connection to false failed: Internal error: Process exited with
  exit status: 1`, `the ACP connection to no-such-command-xyz failed: Internal
  error: No such file or directory (os error 2)`, and
  `the ACP connection to <path>/ur-fake-server failed: Internal error: Process
  exited with signal: 15 (SIGTERM)`.
- After both fixes: `cargo fmt --all -- --check`,
  `cargo test --workspace --all-targets --all-features`,
  `cargo build --workspace --all-features`,
  `cargo clippy --workspace --all-targets --all-features -- -D warnings`, and
  `pnpm -C app build` passed.

## Verdict

The implementation matches the plan. Snapshot-then-live ordering, the
operation guard, updates kept right after `session/new`, and permission
rejection hold as designed, and the tests cover them. The medium finding, a
server that never answers `initialize` blocking every request, is fixed in
`acp.rs`. The low finding, error wording, is fixed in `acp.rs` and `mod.rs`.
