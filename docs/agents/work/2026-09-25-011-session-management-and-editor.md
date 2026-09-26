# Session management and editor

## Plan

`docs/agents/plans/2026-09-25-011-session-management-and-editor.md`

## Summary

The daemon has `delete_session`, which cancels a running turn and holds the operation guard through
`session/delete`, and `set_config_option`. It reports capabilities through watch and config options
through subscribe. The GUI has the sidebar `+` with the folder picker, the workspace and session
menus with their confirmations, the session header, the command list, config pickers, the usage
indicator, and image drops with chips and thumbnails. The fake server has image prompts, delete, the
`pace` config option, and the `pace` and `usage` scripts. The plan's goal is met in code, but the
todo item's check with Ox has not been run, so the item stays unchecked in `todo.md`.

Test guarantees gained: the six daemon tests the plan names;
`the_latest_commands_and_usage_are_kept`, `config_options_come_from`, and
`a_user_message_keeps_its_images` in `reduce.test.ts`; `a_deleted_session_leaves_the_sidebar` and
`the_capabilities_come_from_the_snapshot_and_later_changes` in `watch.test.ts`; `slash.test.ts` and
`usage.test.ts`. Moved: the image case left `a_user_prompt_joins_its_text_parts` for
`a_user_message_keeps_its_images`, and the `available_commands_update` case of
`other_updates_are_ignored` became `config_option_update`, since commands are now kept. None lost.
End-to-end tests gained: the six in `editor.test.ts` and three in `session.test.ts` that the plan
names; the existing end-to-end tests are unchanged.

## Departures from the plan

- `WatchSnapshot.capabilities` and `CapabilitiesChanged.capabilities` are `Box<AgentCapabilities>`.
  Unboxed, `Event` is large enough that clippy's `large_enum_variant` fails on `DaemonMessage`.
  `Entry::Update` boxes `SessionUpdate` for the same reason. The JSON and the TypeScript bindings
  are the same either way.
- `ur wait` stops with "session … was deleted" on `session_deleted`. Without that it would wait
  forever for a deleted session.
- `todo.md` keeps Session management and editor unchecked until the Ox check has been run.

## Decisions

- A disconnect clears `WatchState.capabilities` along with the workspaces and sessions.
- In the command list, inserting a command closes the list until the text changes, so the next Enter
  sends `/<name>` instead of inserting it again.
- `usageText` shows counts below 10k with one decimal ("1.2k"), and shows an amount with its
  currency code when `Intl.NumberFormat` rejects the code.
- The fake server saves each prompt content block as its own `user_message_chunk`, so image prompts
  replay with their images.
- A server-supplied value's description shows under its name in the config picker's list.

## Automated checks

- `make check` — passed: `cargo fmt`, `cargo test` (33 tests in the `ur` binary), `cargo build`,
  `cargo clippy`, `pnpm -C app build`, `pnpm -C app test` (46 cases), and `dprint check`.
- `deleting_*` daemon tests run 30 times — all passed.
- `make e2e` — passed, 34 tests. The first run failed with "No space left on device" while linking.
  Deleting `target/debug` freed space, and the rerun passed.

## Manual verification

1. An ad-hoc check against the fake server: a script outside the repository that imports
   `app/e2e/harness.ts` for the daemon, starts `target/e2e/debug/ur-app` with WebDriver, and sends
   raw wire-protocol requests on the test socket. WebDriver cannot drive the native folder picker,
   menus, or dialogs, so the script sends `add_workspace`, `new_session`, `delete_session`, and
   `remove_workspace` itself, standing in for the folder picker, the workspace menu, the session
   menu, and their confirmations.

   ```sh
   pnpm -C app e2e   # builds target/e2e
   node /tmp/ur-adhoc/check.ts
   ```

   Observed: the "No workspaces" state shows Add Workspace, and the sidebar header shows `+`. The
   session header shows "New session", and its `+` created and selected a second session. The Pace
   picker listed Steady ✓ and Brisk, and choosing Brisk changed its label. Typing `/` showed
   `/tally  count the tallies` with the placeholder "Message agent — / for commands"; Enter inserted
   `/tally`, and a second Enter sent it. `usage` drew the ring at 15%, and hovering showed "1.2k /
   8k tokens (15%)" and "Cost $0.25". WebDriver's `moveTo` sends no pointer events to the webview
   here, so the hover was a synthetic `mouseover`. Dropping `dot.png` and `notes.txt` gave a
   `dot.png` chip and "notes.txt is not an image."; Send got "you said: (1 image)" and a thumbnail
   in the user message. Deleting a session holding a `tool` permission request removed its row and
   showed "Select a session". Removing the workspace brought back "No workspaces".

## Follow-up work

- Run the todo item's check with Ox entirely in the GUI, including the folder picker, both menus,
  and both confirmations, then check off Session management and editor in `todo.md`.
