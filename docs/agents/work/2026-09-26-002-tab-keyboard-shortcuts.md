# Tab keyboard shortcuts

## Plan

`docs/agents/plans/2026-09-26-002-tab-keyboard-shortcuts.md`

## Summary

The tab shortcuts now close and reopen tabs in the active pane, with in-memory history for the
current GUI run. New Terminal uses ⇧⌘N, and the README lists the shortcuts. The plan's goal is met.

## Decisions

- Built on the uncommitted New Session and New Terminal shortcut work already in the working tree
  when this plan was started.
- Dockview's panel removal event excludes tab moves. Layout marks removals caused by missing
  sessions or terminals so they are not recorded as user closures.

## Automated checks

- The first `make e2e` run found that WebDriver serializes an omitted shortcut target as `null`; the
  test helper was corrected before rerunning.
- `make e2e` — 70 passed.
- `make check` — passed.
- `make format-docs` — completed.

## Manual verification

1. Reviewed the complete test diff and checked for whitespace errors with
   `git diff -- app/src/keys.test.ts app/e2e/panes.test.ts app/e2e/harness.ts` and
   `git diff --check`. The shortcut mapping unit test and nine GUI tests were added; no test
   ownership was removed or moved.
