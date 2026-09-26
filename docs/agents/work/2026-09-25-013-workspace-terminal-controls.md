# Workspace terminal controls

## Plan

`docs/agents/plans/2026-09-25-013-workspace-terminal-controls.md`

## Summary

Terminals now belong to a workspace. `open_terminal(workspace)` starts a login shell in the
workspace path, and watch lists every terminal with its terminal title. `detach_terminal` and
`close_terminal` are new requests. Each workspace in the sidebar lists its terminal rows after its
sessions, the workspace menu has New Terminal, the terminal menu has Close Terminal, and a selected
terminal shows the terminal header above its xterm.js view. The global Terminal row and the GUI
state file's one terminal are gone. The plan's goal is met.

Test guarantees:

- Gained owning tests: `terminal_starts_in_its_workspace`, `watch_shows_terminals_and_their_titles`,
  `close_terminal_stops_its_programs`, `removing_a_workspace_stops_its_terminals`,
  `a_detached_connection_gets_no_output`, `terminals_follow_watch_events`, and
  `workspace_terminals_keep_their_opening_order`.
- Moved: "attaching an unknown terminal is an error" from
  `attaching_an_unknown_terminal_is_an_error` to the table-driven
  `unknown_terminals_and_workspaces_are_errors`, which also covers detach, close, and an unknown
  workspace.
- Strengthened: `shell_exit_ends_the_terminal` checks that a watching socket connection gets
  `terminal_exited`, and `a_disconnect_clears_the_watch_state` includes a terminal.
- Gained end-to-end tests: the five in `terminal.test.ts` that the plan names.
- No guarantee lost its owning test. The empty states test reaches No workspaces by removing `home`,
  and keeps its guarantee; the other end-to-end guarantees are unchanged.

## Decisions

- `TerminalPane` sends `detach_terminal` only after its `attach_terminal` settles. A view unmounted
  while attaching would otherwise have its detach handled first, which would leave the attachment in
  place.
- `TerminalPane` sends `terminal_resize` only once attached, so the attachment's own size cannot
  overwrite a later resize. Input goes straight through, since the terminal ID is known on mount.
- `Link::detach()` answers `Ok` while disconnected, because the daemon has already dropped the
  attachment.
- `ops::remove_workspace()` logs, and otherwise ignores, a `close()` error for a terminal whose
  shell exited after `State` removed it.
- The end-to-end harness waits for a `.workspace` element instead of `.sidebar` before deciding
  whether a terminal is shown. The terminal view now depends on the watch snapshot, and the sidebar
  can render before the snapshot arrives. The reopened GUI would then have opened a second terminal.
- Deleted `~/.local/state/ur/gui.json`, which held the old `{ "type": "terminal" }` selection that
  no longer parses.

## Automated checks

- `make check` — passed.
- `make e2e` — passed, 40 tests.

## Manual verification

1. The todo item's check, as a script in `/tmp` that imports `TestEnvironment` and `waitFor` from
   `app/e2e/harness.ts` and drives its own `ur-app` over WebDriver, against the build in
   `target/e2e`. It added a `site` workspace whose `package.json` has a `dev` script that prints
   `tick` every 500 ms, opened a terminal in `home` and one in `site` through the `request` command,
   ran `echo in-home` in the first and `printf '\033]2;devserver\007'; npm run dev` in the second,
   switched between them, killed and reopened `ur-app`, and then sent `close_terminal` for the
   `site` terminal.

   ```sh
   make e2e   # builds target/e2e
   cd app && node /tmp/ur-adhoc/check.ts
   ```

   The sidebar showed `home: >_ sh` and `site: >_ sh`. After the `printf`, the header and the `site`
   row read `devserver`. Selecting the `home` row showed `in-home` with one xterm.js on the page,
   and selecting the `site` row again showed the ticks. After the reopen, the `site` terminal was
   selected, its header read `devserver`, the ticks kept advancing, and `pgrep -f ur-adhoc-tick`
   found the same process. After `close_terminal`, only `home: >_ sh` remained, the main area showed
   "Select a session", and `pgrep` found no process. A screenshot matched the wireframe's sidebar
   rows and terminal header.
