# Tab keyboard shortcuts

## Goal

Close the active tab with Command+W on macOS or Ctrl+W on other platforms, and reopen recently
closed tabs with Command+Shift+T or Ctrl+Shift+T in reverse closing order. Keep that history only
for the current app run. Move New Terminal from Command+T to Command+Shift+N, retain Command+N for
New Session, and add a README table of the app's keyboard shortcuts.

## Related code

- `app/src/App.tsx` owns the Dockview API, selection, and existing new-tab keyboard listener.
- `app/src/components/Layout.tsx` restores and saves the layout, removes tabs for missing sessions
  and terminals, and can observe tab removals from both Close Tab and the keyboard shortcut.
- `app/src/keys.ts` maps keyboard events to the existing New Session, New Terminal, and permission
  actions.
- `app/src/layout.ts` provides `TabItem`, `tabId()`, `openTab()`, and `tabWorkspace()`.
- `app/e2e/panes.test.ts` tests tab behavior through the GUI, using `Gui.pressShortcut()` in
  `app/e2e/harness.ts`.

## Decisions

- Use Command on macOS and Ctrl on other platforms for close and reopen. Keep the existing Command+N
  New Session shortcut; change its New Terminal companion to Command+Shift+N. Match the exact
  modifiers so a terminal's Ctrl+W on macOS remains available to its shell.
- Keep recently closed tabs in memory in `App`, which stays mounted during a daemon reconnect.
  Observe actual tab removals in `Layout` so both Close Tab and Command/Ctrl+W enter the same
  history. Do not record tabs removed because their session or terminal disappeared, or during
  saved-layout restoration. Dockview tab moves must not count as closures.
- Reopen in the active pane, using `openTab()`. Skip a closed tab if its session or terminal no
  longer exists or its tab has already been reopened by another action. Closing or reopening a tab
  never closes its session or terminal. Ignore held-key repeats for these actions.
- There is no README yet; create `README.md` with a short description and one table covering New
  Session, New Terminal, Close Tab, Reopen Closed Tab, and the existing permission shortcuts. Label
  platform-specific shortcuts accurately.

## Naming

- **Tab** — a Dockview panel showing one agent session or terminal, as defined in
  `docs/agents/glossary.md`.
- **Active pane** — the Pane currently active in Dockview, which receives a reopened tab.
- **Recently closed tabs** — an in-memory list of `TabItem`s closed by the user during the current
  app run, ordered from oldest to newest so reopening takes the newest eligible one.

## Test plan

- Update `app/src/keys.test.ts` for Command+Shift+N replacing Command+T, exact modifier matching,
  and the platform-specific close and reopen shortcuts.
- In `app/e2e/panes.test.ts`, verify Command/Ctrl+W closes only the active tab, including with the
  editor or terminal focused; the session or terminal remains in the sidebar. Verify both shortcut
  and Close Tab button closures can be reopened in reverse order in the active pane, and that the
  history is empty after an app restart.
- Verify a removed session or terminal and a tab already reopened from the sidebar are skipped by
  the reopen shortcut. Verify Command/Ctrl+Shift+T with no eligible tab leaves the layout alone.
  Update `Gui.pressShortcut()` to issue Ctrl combinations where the test needs them.
- Keep the existing GUI tests for layout restoration, tab moves, and permission shortcuts. Add these
  user-visible guarantees to `docs/agents/testing.md`.

## Implementation plan

1. Update `app/src/keys.ts` and `app/src/keys.test.ts` to recognize the new terminal binding and the
   platform-specific close and reopen bindings.
2. Keep the recently closed tabs in `app/src/App.tsx`. Extend its capture-phase key listener to
   close the active Dockview panel or reopen the newest eligible `TabItem`. Pass a callback to
   `Layout` for recording user-closed tabs.
3. In `app/src/components/Layout.tsx`, report user tab removals to `App` while excluding the
   component's own cleanup for missing sessions and terminals. Keep layout saving and active
   selection updates working for close and reopen.
4. Extend `app/e2e/harness.ts` and `app/e2e/panes.test.ts` with the shortcut and recently closed tab
   coverage.

## Documentation updates

- Create `README.md` with the shortcut table.
- Update `docs/agents/architecture.md` with the shortcut behavior and in-memory reopening rule,
  `docs/agents/testing.md` with the new end-to-end guarantees, and `AGENTS.md` with the README map
  and changed file descriptions.
