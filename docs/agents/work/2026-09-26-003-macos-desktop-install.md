# Install and launch ur as a macOS app

## Plan

`docs/agents/plans/2026-09-26-003-macos-desktop-install.md`

## Summary

The app bundles the daemon, starts it when needed, and lets a user configure an ACP server in the
GUI. The daemon keeps sessions and terminals after the app closes. A GitHub Actions workflow builds,
checks, and publishes the unsigned Apple Silicon DMG without an Apple developer account.

## Departures from the plan

- Removed signing and notarization because the user will not use an Apple developer account. The
  plan was updated to specify an unsigned DMG and the README explains macOS's manual first open.
- Used `hdiutil` to create the DMG after Tauri built the app bundle. Tauri's DMG helper waited on
  Finder AppleScript in this environment.
- Build the release artifact on GitHub's macOS runner and publish only that artifact. The locally
  built DMG was used for smoke testing and was not uploaded.
- Started the sidecar as a detached process beside the app executable, so it survives app exit.
- Tried a hidden window for the macOS end-to-end suite. WebKit did not render Dockview's session
  pane, so the experiment was reverted. The suite remains visible and focuses its windows.

## Automated checks

- `make check` — passed: Rust formatting, tests, build, Clippy, webview build and tests, and
  Markdown.
- `make e2e` — passed: 77 tests, including daemon startup, server setup and recovery, terminal use
  after server failure, and reopening the app.
- `scripts/build-macos` — passed: built the unsigned app and DMG; the package checker found the
  daemon sidecar and `hdiutil verify` accepted the DMG.
- `.github/workflows/release-macos.yml` runs `make check`, `make e2e`, and `scripts/build-macos`
  before attaching the runner-built DMG to the release.
- `git diff --check` — passed.

## Manual verification

1. Mounted a built DMG, copied `ur.app` outside the repository, and launched it with an isolated
   socket, home, and config directory. After closing the app, a framed `watch` request on the socket
   confirmed that its bundled daemon was still running. The daemon was then stopped.

## Test guarantees

- Added GUI tests for app-managed daemon startup, failed and corrected ACP server choices, saved
  session recovery after reopening, and terminal use while the server is unavailable.
- Added a watch-store test for server state snapshots and changes. Existing tests were updated for
  the new protocol fields. No prior guarantee or owning test was removed.
