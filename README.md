# ur

ur is a desktop client for ACP agents. It keeps sessions and terminals available across app restarts
while its daemon is running.

## Install on macOS

Download the unsigned Apple Silicon DMG from
[GitHub Releases](https://github.com/kkestell/ur/releases/latest), or build it from this repository
with `scripts/build-macos`. A local build writes the DMG to `target/release/bundle/dmg/`. Open it
and drag `ur.app` to Applications. No Apple developer account, signing certificate, or paid service
is needed to build it. Because the app is unsigned, macOS may block its first launch. After trying
to open it, go to System Settings → Privacy & Security and choose **Open Anyway** for ur, following
[Apple's instructions](https://support.apple.com/en-us/102445).

Open ur, choose the executable of an installed ACP server, and add each argument in its own field.
Then add a workspace with the **+** beside Workspaces, create a session, and send a prompt. ur
starts its bundled daemon automatically. Closing the window leaves the daemon and terminals running;
reopening the app reconnects to them. The ACP server is installed separately.

## Keyboard shortcuts

| Action            | macOS                  | Other platforms  |
| ----------------- | ---------------------- | ---------------- |
| New Session       | Command+N              | Ctrl+N           |
| New Terminal      | Command+Shift+N        | Ctrl+Shift+N     |
| Close Tab         | Command+W              | Ctrl+W           |
| Reopen Closed Tab | Command+Shift+T        | Ctrl+Shift+T     |
| Allow once        | Command+Y              | Ctrl+Y           |
| Always allow      | Command+Shift+Y        | Ctrl+Shift+Y     |
| Reject once       | Command+Option+Z       | Ctrl+Alt+Z       |
| Always reject     | Command+Shift+Option+Z | Ctrl+Shift+Alt+Z |

Permission shortcuts answer the active session's oldest pending request when it has the matching
option.
