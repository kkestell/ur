# Workspace colors

## Plan

`agents/plans/2026-09-26-005-workspace-colors.md`

## Summary

Every workspace now has a workspace color, chosen from fourteen Catppuccin Mocha swatches after the
folder picker. Tabs have a darker background in their workspace color, and each tab and its content
have a 1px border in that color. The plan's goal is met.

## Decisions

- The workspace color picker lays out its swatches in a grid of seven columns, two rows for the
  fourteen colors.
- The architecture's source layout listing also gained `AddWorkspaceButton`, `tabColor()`, and
  `colors.ts`, alongside the documentation updates the plan named.

## Automated checks

- `make check` — Passed.
- `make e2e` — Passed, 86 tests, including the new
  `adding a workspace asks for its workspace color`.

## Manual verification

1. Tab and content colors and the workspace color picker, through the WebDriver build in
   `target/e2e`. An ad-hoc script started the test environment, added a second workspace `other`
   with the color mauve, created a session in `home` (blue), opened a terminal in `other`, opened
   both tabs, and took a screenshot; then it answered the folder picker and clicked the sidebar's
   `+` and took a second screenshot.

   ```sh
   make e2e
   cd app && node e2e/artifacts/workspace-colors.ts
   ```

   The session tab had a dark blue background and blue border, the terminal tab a dark mauve
   background and mauve border, and the terminal's content a mauve border whose top edge ran under
   both tabs. The tab bar showed no other line under the tabs. The picker appeared centered over a
   dimmed window, showing the folder name and fourteen swatches in swatch order.
