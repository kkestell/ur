# Agent message links

## Plan

`docs/agents/plans/2026-09-25-010-agent-message-links.md`

## Summary

`tauri-plugin-opener` joins the core, and `AgentMessage` renders every link with `target="_blank"`,
so the opener plugin's script opens http, https, mailto, and tel links in their default application
and leaves the app in place. Other links do nothing. Links show in `#74ade8`. The plan's goal is
met, and OX-0007 is fixed.

Test guarantees: `clicking a link in an agent message leaves the app in place` in `thread.test.ts`
owns the new guarantee. It fails on the code before this change, where the click replaced the app.
No guarantee was lost or moved.

## Decisions

- The app uses react-markdown 10, not 9 as the plan says. It has no `linkTarget` either, so the
  `components` option is still the way to set `target`.

## Automated checks

- `make check` — passed: `cargo fmt`, `cargo test`, `cargo build`, `cargo clippy`,
  `pnpm -C app build`, `pnpm -C app test` (25 cases), and `dprint check`.
- `make e2e` — passed, 25 tests.

## Manual verification

1. The plan's ad-hoc check, through the WebDriver build in `target/e2e`. A script started a daemon
   in a temporary directory with the fake server, added a workspace, created a session, and prompted
   it with `ur`, then opened `ur-app`, selected the session, and clicked each link through
   WebDriver.

   ```sh
   make e2e
   ur workspace add demo "$HOME"
   ur prompt "$(ur new demo)" 'see [the example](https://example.com/) and [main](src/main.rs)'
   ```

   Both links rendered with `target="_blank"` in `#74ade8`. Clicking the first was cancelled by the
   opener plugin's script (`defaultPrevented` in a later click listener), and clicking the second
   did nothing; after each, the webview stayed at `tauri://localhost` with the sidebar and thread in
   place. `plugin:opener|open_url` called from the webview succeeded for `https://example.com/` and
   was refused for `file:///etc/hosts` with "Not allowed to open url". The user confirmed that
   clicking a link opens it in the browser.
