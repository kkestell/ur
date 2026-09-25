# One-shot ACP client

## Plan

`docs/agents/plans/2026-09-25-003-one-shot-acp-client.md`

## Summary

`ur agent-run <workspace> <prompt>` reads the `[server]` table from the config file, runs one
prompt, prints each line of output the plan describes, answers permission requests from stdin, and
ends with the stop reason. The plan's goal is met: the check in `docs/agents/todo.md` passes with
Ox. The three tests in the plan own new guarantees in `crates/ur/src/one_shot.rs`; no existing test
gained, lost, or moved a guarantee.

## Departures from the plan

- `crates/ur/Cargo.toml` enables tokio's `io-std` and `io-util` features for stdin and `read_line`.
- `Cargo.lock` pins `agent-client-protocol` to 2.1.0. The requirement `2.1.0` resolved to 2.2.0, and
  `docs/agents/architecture.md` and the plan name 2.1.0.
- The test agent also records the permission outcome and whether it received `session/cancel`.
  `answers_permission_from_input` asserts both, so the end-of-input case checks that the
  cancellation was sent.

## Decisions

- `run`, not `start`, makes the workspace absolute, so the test sees the path the server receives.
- `run` prints the initialize response before checking its protocol version, so an unsupported
  version is visible in the output as well as the error.

## Automated checks

- `cargo fmt --all -- --check` — passed.
- `cargo test --workspace --all-targets --all-features` — passed.
- `cargo build --workspace --all-features` — passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — passed.
- `pnpm -C app build` — passed.
- `pnpm -C app e2e` — passed.

## Manual verification

Setup, with a temporary config directory:

```sh
mkdir -p /tmp/ur-check/config/ur /tmp/ur-check/ws
printf '[server]\ncommand = "ox"\n' > /tmp/ur-check/config/ur/config.toml
touch /tmp/ur-check/ws/alpha.txt /tmp/ur-check/ws/beta.txt
```

1. The check in `docs/agents/todo.md`: approving the `ls` permission request.

   ```sh
   echo 1 | XDG_CONFIG_HOME=/tmp/ur-check/config target/debug/ur agent-run \
     /tmp/ur-check/ws 'Run the shell command `ls` in this directory and tell me what it prints.'
   ```

   Ox stopped at a permission request with `1. Approve (allow_once)` and `2. Deny (reject_once)`.
   After the answer, the output showed the tool call completing with `alpha.txt` and `beta.txt`, the
   answer, and `end_turn` as the last line. The exit status was 0, and nothing from the server's
   stderr appeared.

2. End of input cancels the turn: the same command with `</dev/null` instead of `echo 1 |`. The tool
   call update reported `failed`, and the last line was `cancelled`.

3. A missing config file: `XDG_CONFIG_HOME=/tmp/ur-check/none target/debug/ur
   agent-run . hi`
   failed with `reading /tmp/ur-check/none/ur/config.toml` and exit status 1.
