# Install and launch ur as a macOS app

## Goal

A macOS user downloads a DMG, drags ur to Applications, and opens it without installing a CLI or
starting a daemon. The app starts the bundled daemon when needed, reconnects to one already running,
and keeps the daemon and its terminals running after the window closes. A user can choose an ACP
server in the app, add a workspace, create a session, and send a prompt. The README describes this
flow once a downloadable build exists.

## Related code

- `app/src-tauri/tauri.conf.json` and `app/src-tauri/src/main.rs` — the app bundle and startup.
- `app/src-tauri/src/link.rs` — retries the daemon socket and restores the GUI's desired set.
- `crates/ur/src/main.rs` and `crates/ur/src/daemon/mod.rs` — `ur daemon` and its startup.
- `crates/ur/src/config.rs` and `crates/ur/src/daemon/acp.rs` — the JSON server configuration and
  the ACP connection's supervisor.
- `crates/ur-client/src/protocol.rs` and `crates/ur/src/daemon/server.rs` — daemon requests and
  watch events.
- `app/src/App.tsx` and `app/src/components/` — the current connection and empty states.
- `app/e2e/harness.ts` — starts the daemon before the GUI in the existing tests.

## Decisions

- Package macOS first. Build the `ur` binary for the same target as the Tauri app and bundle it as a
  Tauri sidecar. The GUI starts `ur daemon` only when it cannot connect to the socket. If another
  app instance starts it first, the later instance connects to that daemon. Closing the GUI does not
  stop the daemon; the next launch reconnects to it. Keep `ur daemon` available for development and
  testing, but do not require users to invoke it.
- Keep one JSON config file owned by the daemon. Add a daemon request that saves the server command
  and argument list and starts or restarts the ACP connection. Expose the current server state to
  the GUI so a missing executable or failed connection has an actionable message. The daemon can
  start without a config file, as it does today. The GUI offers a file picker for the server
  executable and a separate input for each argument, so a macOS app does not depend on a shell's
  `PATH` or on splitting a command string.
- Keep the server generic: the app does not ship Ox or interpret Ox's config file. The selected
  server must be installed separately. Use the existing workspace and session controls after the
  server is connected.
- Produce an unsigned DMG that can be built and distributed without an Apple developer account.
  Document macOS's manual first-open step and do not require signing or notarization. Tauri
  documents both [sidecar bundling](https://v2.tauri.app/develop/sidecar/) and
  [DMG distribution](https://v2.tauri.app/distribute/dmg/).

## Naming

- **Daemon** — the long-running `ur daemon` process, as in `docs/agents/glossary.md`.
- **Server** — the configured ACP process the daemon launches, as in `docs/agents/glossary.md`.
- **Server state** — the daemon's current server configuration and connection error, sent with the
  watch snapshot and changed by a watch event. Use this term in the wire protocol and GUI.

## Test plan

- Launch the app with no daemon. It starts the bundled daemon and reaches the server setup screen.
- Choose the fake server in the app. The daemon saves `config.json`, connects, and can create a
  session without restarting the app. An invalid executable shows the failure and can be corrected.
- With a running terminal, close and reopen the app. The same daemon, terminal, and screen remain.
  Opening a second app window does not start another daemon.
- Build a macOS app bundle and unsigned DMG. Install the bundle outside the repository and repeat
  the first launch and reopen checks. Verify the sidecar is inside the bundle.

## Implementation plan

1. Add a build step for the `ur` sidecar in `scripts/` and package it through
   `app/src-tauri/tauri.conf.json`. Update `app/src-tauri/Cargo.toml` for the Rust sidecar API.
2. In `app/src-tauri/src/main.rs` and `app/src-tauri/src/link.rs`, connect to an existing daemon or
   start the sidecar once when the socket is unavailable. Keep the daemon alive after app exit and
   report launch failures to the webview.
3. In `crates/ur/src/config.rs`, `crates/ur/src/daemon/acp.rs`, `crates/ur/src/daemon/state.rs`,
   `crates/ur/src/daemon/server.rs`, and `crates/ur-client/src/protocol.rs`, add the request to save
   and apply server configuration and add server state to watch. A failed server launch leaves the
   daemon available for another configuration attempt.
4. Add the server setup controls to `app/src/App.tsx`, `app/src/components/`, and `app/src/ipc/`.
   Show server state in the GUI and use the native file picker for the executable.
5. Extend `app/e2e/harness.ts` and the GUI tests to cover app-managed daemon startup, server setup,
   and persistence after the window closes. Add a macOS package smoke check in `scripts/`.
6. Add a GitHub Actions release workflow that builds and checks the unsigned DMG on a macOS runner
   and publishes that runner-built artifact for a version tag. Then add the download, installation,
   manual first-open, and first-use steps to the README.

## Documentation updates

- Update `README.md` for the released desktop workflow.
- Update `docs/agents/architecture.md`, `docs/agents/glossary.md`, `docs/agents/testing.md`, and
  `AGENTS.md` for daemon startup, server state, GUI setup, and packaged files.
