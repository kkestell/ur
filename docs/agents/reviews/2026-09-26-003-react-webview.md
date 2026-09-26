# React webview

## Scope and coverage

Reviewed the webview's React and TypeScript in `app/src`: the `watch` and `sessions` stores, the
transcript reducer and permission placement, `layout.ts`, the IPC layer, `App`, and every component
(`Layout`, `Sidebar`, `Tab`, `StatusMark`, `Thread`, `AgentMessage`, `ToolCall`, `ToolCallContent`,
`Permission`, `Editor`, `ConfigPicker`, `UsageIndicator`, `Popover`, `TerminalPane`), plus their
unit tests and the `Editor` and pane end-to-end tests. Used the correctness, simplicity,
readability, resources, and testing lenses.

The GUI was exercised through the end-to-end suite, which was run in full. Native menus and the
WebDriver focus bridge were not driven directly; the file-attachment paths were traced, not
reproduced by hand. The model-description special case is already tracked as OX-0012 and is not
duplicated here. Existing issues OX-0009 and OX-0011 were left as they are.

## Fixed

- **A press outside a config picker needed two presses to close it**
  (`app/src/components/Popover.tsx:78`): `useClickOutside` set its `inside` flag on any press within
  the element, including the press that opened the popover. That press runs before the window
  `mousedown` listener is installed, so the flag was never cleared, and the next press anywhere was
  treated as inside. Opening a picker or the More menu and then pressing elsewhere once left it
  open; a second press closed it. The handler now marks a press as inside only while `open`.
  Verified with an ad-hoc WebDriver run: before the fix the list survived the first outside press
  and closed on the second; after the fix it closes on the first. Added `Gui.press()` in
  `app/e2e/harness.ts`, which sends `mousedown`, `mouseup`, and `click` (the existing `Gui.click()`
  sends only `click` and cannot exercise a press), and the test "a press outside a config picker
  closes it" in `app/e2e/editor.test.ts`.

## Findings

### Low

#### Correctness

- **OX-0013 A config picker reopens with its previous filter**
  (`app/src/components/ConfigPicker.tsx:52`): `SelectPicker` keeps the filter text in `filter`.
  Choosing a value clears it, but closing the popover — by pressing outside it or by pressing its
  button again — does not. Reopening the picker shows the old filter text and an already-filtered
  list, so a picker opened before may show fewer values than the user expects until the field is
  cleared. Evidence: `choose()` resets `filter`, and the
  `useClickOutside(open, () => setOpen(false))` and `setOpen(!open)` paths do not; only a value
  choice clears it. Fix: reset `filter` when the popover opens (or when it closes), so every open
  starts from the full list.

## Checks run

- `make check` — passed (`cargo fmt`, `cargo test`, `cargo build`, `cargo clippy`, `pnpm build`,
  `pnpm test`, `dprint check`).
- `make e2e` — passed, 57 tests (including the new regression test).
- Ad-hoc WebDriver check against the built app — reproduced the two-press behavior before the fix
  and confirmed the one-press close after it.

## Verdict

The React code follows its contracts and its reducer, store, and layout logic are consistent with
the architecture. One real interaction bug — a press outside a picker not closing it — was found and
fixed, with an end-to-end test. One low-severity polish issue is left open. No high or medium
severity finding remains.
