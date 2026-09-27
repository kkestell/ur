# Minimal TUI and tmux

## Plan

[Minimal TUI and tmux](../plans/2026-09-27-001-minimal-tui-and-tmux.md)

## Decisions

- Use the ACP SDK's child-process ownership and stderr callback; diagnostics enter the same event
  queue as ACP updates. No separate process supervisor is needed.
- Normalize CRLF and CR in bracketed paste to LF. The tmux test exposed tmux's conversion of pasted
  line breaks to carriage returns.
- The existing user config still contains IDs and icons. Live verification used a temporary reduced
  config, leaving the user's file untouched. Replace that config as documented in `README.md` before
  using the new binary normally.

## Automated checks

- `make check` — passed: 15 unit tests, one CLI startup test, build, Clippy, and formatting. The two
  tmux tests are deliberately ignored here and run by `make e2e`.
- `make e2e` — both tmux tests passed. Early runs exposed the paste normalization issue and an
  incorrect expected fake-server reply prefix; both were corrected.
- The first ACP exit test expected success from the SDK's outer connection result even when the
  server failed. It now closes a separate in-process server while permissions are pending and checks
  that the connection releases the pending work within a bounded wait.
- `git diff --check` — passed.
- Test ownership: direct prompting, permission IDs, version negotiation, cancellation, and turn
  errors moved from one-shot/daemon tests to ACP tests. Terminal editing, escaping, and partial tool
  updates gained focused unit tests. Actual keys, paste, resizing, reconnect, scrollback, adjacent
  shells, and terminal restoration belong to isolated tmux tests. GUI, wire framing, persisted
  workspaces, history loading, supervision, and application-hosted terminals lost their guarantees
  with the deleted implementations.

## Manual verification

Used the installed `/Users/kyle/.local/bin/ox` with no arguments, an empty temporary workspace, and
a separate tmux socket. To reproduce, create a temporary reduced config naming that executable,
then:

```sh
tmux -L ur-live -f "$PWD/tmux.conf" new-session -s ur-live
XDG_CONFIG_HOME=/path/to/temporary/config target/debug/ur /path/to/temporary/workspace
```

1. Split an adjacent shell pane. Ask Ox to run `pwd` without modifying files.
2. At the permission request, detach and reattach with `tmux -L ur-live attach -t ur-live`; approve
   option 1. The directory result and earlier output remained readable.
3. Ask Ox to run `sleep 20`, approve it, then press Ctrl-C while it runs. Ox reported cancellation,
   the turn ended, and the prompt returned.
4. Use the adjacent shell, then press Ctrl-D in ur and run a shell command there. Both shells were
   usable. The ur process and its direct server child had exited. Stop only the test tmux server
   with `tmux -L ur-live kill-server`.

No live checks were skipped. The implementation and acceptance check are complete.
