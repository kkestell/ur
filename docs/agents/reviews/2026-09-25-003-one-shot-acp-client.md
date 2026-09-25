# One-shot ACP client review

## Scope and coverage

Reviewed commit `b47986c`, the One-shot ACP client section of `docs/agents/todo.md`:
`crates/ur/src/one_shot.rs`, `crates/ur/src/config.rs`, `crates/ur/src/main.rs`, the Cargo
manifests, and the documentation changes, against
`docs/agents/plans/2026-09-25-003-one-shot-acp-client.md`. Also read the SDK's `AcpAgent` process
handling (`agent-client-protocol-2.1.0/src/acp_agent.rs`) for how the server's stdio, stderr, and
exit are handled.

Lenses: correctness, error-handling, resources, testing, api-design, simplicity, documentation,
rust-idioms, rust-cargo.

## Findings

### Low

#### Error-handling

- **`agent-run` hangs after the server exits during a permission request**
  (`crates/ur/src/main.rs:24`): If the server exits while `agent-run` is waiting for a permission
  answer, `run` returns the error at once, but the process does not print it or exit until the user
  enters another line. Tokio reads stdin with a blocking read on a separate thread that cannot be
  cancelled, and dropping the runtime at the end of `#[tokio::main]` waits for that read. Reproduced
  with a server script that sends `session/request_permission` and exits 0.5 seconds later, with
  stdin held open for 6 seconds: `run` returned after about one second, and the error appeared after
  six. Fix: build the runtime in `main` and end it with `Runtime::shutdown_background()`, which does
  not wait for the read. Not covered by an automated test: the behavior is in the process's
  shutdown, outside `run`, and needs a launched server process.

## Checks run

- The reproduction above, before and after the fix. After the fix, the error appears when the server
  exits.
- The check in `docs/agents/todo.md` with Ox, after the fix: approving the `ls` permission request
  printed the tool call and the answer, the last line was `end_turn`, and the exit status was 0.
- `ox </dev/null` exits, so the server does not outlive `agent-run` when it is interrupted with
  Ctrl-C, even though the SDK starts it in its own process group.
- `cargo fmt --all -- --check` — passed.
- `cargo test --workspace --all-targets --all-features` — passed.
- `cargo build --workspace --all-features` — passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — passed.
- `pnpm -C app build` — passed.
- `pnpm -C app e2e` — passed.

## Verdict

The one-shot client meets its plan and passes the check in `docs/agents/todo.md`. The one finding, a
hang after the server exits during a permission request, is fixed in `main.rs`.
