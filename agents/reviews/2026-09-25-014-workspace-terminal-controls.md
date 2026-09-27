# Workspace terminal controls

## Scope and coverage

Commit `f1291e1`, "List and manage terminals under their workspaces", against its plan
`plans/2026-09-25-013-workspace-terminal-controls.md` and the Workspace terminal controls item in
`todo.md`. Reviewed the daemon's `Terminals` and its reports to `State` (`terminal.rs`, `state.rs`,
`ops.rs`, `server.rs`, `mod.rs`), the wire protocol changes, the core's `Link::attach()` and
`Link::detach()` and the terminal commands, the GUI state file, the webview's watch reducer,
`TerminalPane`, `App`, the sidebar rows and menus, the tests in `crates/ur/tests/terminal.rs` and
`watch.test.ts`, the end-to-end harness, and the documentation changes. Traced into
`ur_client::Client`'s routing of `terminal_exited`, portable-pty 0.9.0's `ProcessSignaller::kill()`,
and vt100 0.16.2's OSC 0 and OSC 2 handling.

Lenses: correctness, concurrency, resources, testing, comments, and documentation.

The GUI's attach and detach ordering was checked by tracing, not by running the app. The end-to-end
suite was not run; the work log records `make e2e` passing on this commit.

This commit also resolves OX-0005: the attach error now lives in `TerminalPane`, which unmounts
while the GUI is disconnected and mounts with no error after it reconnects. Its status is set to
`fixed`.

## Fixed

- **Doc comments left unwrapped** (`crates/ur/src/daemon/state.rs:19`,
  `crates/ur-client/src/protocol.rs:37`): the `State` doc comment had one line of about 140 columns,
  and the `Watch` doc comment broke its paragraph mid-sentence. Rewrapped both.

## Findings

No open findings.

## Checks run

- `cargo test -p ur --test terminal` — passed, 10 tests.
- `cargo fmt --all -- --check` — passed.
- `make check-docs` — passed.

## Verdict

The terminal controls meet the plan and the todo item. The lock order holds on every path, each
socket connection gets `terminal_exited` once, and Close Terminal and Remove Workspace stop the
shell through the same exit path as `exit`. One comment formatting fix; nothing left open.
