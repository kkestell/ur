# Attention in the GUI

## Plan

`docs/agents/plans/2026-09-25-008-attention-in-the-gui.md`

## Summary

The sidebar orders workspaces and sessions by attention, shows a count per workspace, marks each
session's status at the right edge of its row, and bolds unread sessions. The core keeps the visible
sessions and the window focus in the desired set and sends `focus` from them, replayed after the
subscriptions on each connect. Pending permission requests render in the thread from the watch
store, merged with their tool call rows, with one button per option and the shortcuts ⌘Y, ⇧⌘Y, ⌥⌘Z,
and ⇧⌥⌘Z. A rejected prompt's error sits under its user message through a CSS adjacency rule. The
daemon and the wire protocol are unchanged. The plan's goal is met: the check in
`docs/agents/todo.md` passes with Ox.

Test guarantees: the six vitest cases the plan lists and the seven end-to-end tests in
`attention.test.ts` own the new guarantees. The existing `sessions_order_by_last_activity` keeps its
guarantee for sessions that need no attention. The existing end-to-end tests are unchanged.

## Departures from the plan

- `shortcutKind()` takes `Keys`, the five fields it reads from a keyboard event, not a
  `KeyboardEvent`. Vitest runs under node, which has no `KeyboardEvent`, so the test passes plain
  objects. It matches the physical key through `code`, since ⌥ changes `key` on macOS.
- `send_focus()` spawns on `tauri::async_runtime`, not `tokio::spawn`: the window event handler runs
  on the main thread outside the Tokio runtime, and `tokio::spawn` there panicked the app.

## Decisions

- The `set_visible` effect runs before `App`'s early returns, since a hook after them changes the
  hook count between renders and crashes React. `visible` is computed from the selection and the
  watch store first, and the rendered selection derives from it.
- Session rows and workspace names are flex rows with a `label` span, so the status mark and count
  keep their place at the right edge while the label truncates.
- `answerPermission()` in `App.tsx` logs an `error` response and does nothing else, as the plan
  says; the editor shows no message for it.

## Automated checks

- `cargo fmt --all -- --check` — passed.
- `cargo test --workspace --all-targets --all-features` — passed; no bindings changed.
- `cargo build --workspace --all-features` — passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — passed.
- `pnpm -C app build` — passed.
- `pnpm -C app test` — passed, 22 cases.
- `dprint check` — passed.
- `pnpm -C app e2e` — passed, 17 tests. Two earlier runs failed with "timed out waiting for the
  terminal to render" on HEAD as well as on this change; a daemon and fake server left behind by a
  run that panicked were the cause, and the suite passed twice in a row after they were killed.

## Manual verification

1. The check in `docs/agents/todo.md` with Ox, driven through the WebDriver build in `target/e2e` by
   two ad-hoc scripts. Each starts a daemon in a temporary directory with an Ox config file, adds
   workspaces and creates sessions with `ur new`, opens `ur-app`, and drives it through WebDriver.

   ```sh
   make e2e
   node --experimental-strip-types /tmp/ur-adhoc/attention.ts
   node --experimental-strip-types /tmp/ur-adhoc/rejected.ts
   ```

   With sessions in workspaces `alpha` and `beta`, prompting `alpha`'s session to run `ls -la`
   showed the spinner on its row, then the dot, the count 1 on `alpha`, and in the thread the
   request titled `ls -la` with Ox's working-directory content, the rows `✓ Approve ⌘Y` and
   `✕ Deny ⌥⌘Z`, and "Awaiting Confirmation." A ⌘Y keydown dispatched on the focused editor answered
   it: the request left the thread, the spinner returned, and the turn ended with the tool call row
   and the reply. `ur prompt` on `beta`'s session while `alpha`'s was selected made `beta`'s row
   bold with the count 1 and put `beta` first; selecting it cleared the bold and the count. Killing
   Ox during a turn showed `!` on the row and the error "the ACP connection to ox failed: Internal
   error: Process exited with signal: 9 (SIGKILL)" at the end of the thread. In the second script,
   clicking Deny answered a request, and a 7 MB prompt showed "Invalid params: prompt exceeds the
   model context limit" with its top at the user box's bottom edge, and `!` on the row.

## Follow-up work

- The thread does not show which option answered a request once it is resolved; the tool call row
  returns as it was. Thread rendering can show the outcome.
- After a server restart, a reloaded session's interrupted turn shows its user message followed
  directly by the interruption error, since Ox's replay omits the uncommitted agent output. The
  adjacency rule then makes it look like a rejected prompt.
