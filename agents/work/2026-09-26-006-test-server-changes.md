# Test server changes in the daemon

## Plan

`agents/plans/2026-09-26-006-test-server-changes.md`

## Summary

The daemon test setup now routes server changes through `ServerControl` with an isolated config file
and independent in-process ACP launches. Eight tests cover add, rename, removal, launch edits,
pending requests, failed writes, and concurrent startup. OX-0014 is fixed.

## Decisions

- The fake server's load and delete holds share saved sessions across launches but can be reset
  independently for a replacement connection.
- A directory at the config file path makes each write fail without changing process-wide
  environment variables.

## Automated checks

- `cargo test -p ur --bin ur daemon::tests:: --quiet` — 45 daemon tests passed.
- `make check` — Passed after fixing two Clippy findings from the first run.

## Manual verification

The focused daemon suite reproduces server changes through socket requests:

```sh
cargo test -p ur --bin ur daemon::tests:: --quiet
```

The complete test diff adds eight owning tests. No guarantees or owning tests were removed or moved.
