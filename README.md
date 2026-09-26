# ur

ur is a desktop client for ACP agents. It keeps sessions and terminals available when you close and
reopen the app.

## Install on macOS

Download `ur-macos-arm64.dmg` from the
[latest release](https://github.com/kkestell/ur/releases/latest). Open the DMG and drag ur to
Applications. The app is unsigned, so macOS may block its first launch. If that happens, try to open
ur, then go to System Settings → Privacy & Security and choose **Open Anyway**, following
[Apple's instructions](https://support.apple.com/en-us/102445).

## Get started

Install an ACP-compatible agent separately. Open ur, choose the agent's executable, and add any
launch arguments in separate fields. Then add a workspace with the **+** beside Workspaces, create a
session, and send a prompt. Sessions and terminals stay available when you close and reopen ur.

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
