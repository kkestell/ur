# Testing

`make check` runs Rust tests, builds, Clippy, and formatting checks for Rust and Markdown.
`make e2e` builds the fake server and runs the otherwise ignored tmux integration tests. Tests need
no model provider or Ox.

## Where tests go

- ACP behavior belongs in tests beside `acp.rs`, against the in-process fake server: session reuse,
  ordered updates, permission IDs, concurrent requests, cancellation, errors, and disconnection.
- Input editing, Unicode display width, text escaping, and partial tool updates belong beside
  `tui.rs`.
- Terminal behavior belongs in `crates/ur/tests/tui.rs`, against built ur and fake-server binaries
  on an isolated tmux socket. Real keys, bracketed paste, resizing, detach and reattach, scrollback,
  and shell recovery belong here.
- Live checks against an authenticated server are manual. Use the acceptance check in `todo.md`.
  Report a missing or unauthenticated server as a skipped live check.

## Test discipline

Each test owns one durable, observable guarantee. Add tests at the closest stable boundary and
rewrite or remove tests when their guarantees change. Test the same assertion at different layers
only when those layers have distinct failure modes. Use table-driven cases for varied inputs to the
same behavior. Keep fixtures proportional to the guarantee and test behavior rather than internals.

Before finishing changes to tests, inspect the complete test diff and report which guarantees
gained, lost, or moved their owning tests.

## Terminal integration

Each test gets a temporary home, config, state directory, and tmux socket. Use bounded waits and
terminate only that test's tmux server. Failures include the captured pane text. These tests require
tmux on PATH and do not use the user's personal tmux config or server.
