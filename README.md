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

Install an ACP-compatible agent separately. Open ur, choose **Server**, and add a named server with
its executable and launch arguments. Add more servers in the same settings screen. Then add a
workspace with the **+** beside Workspaces, choose its folder, and choose its color. The workspace
and pane creation menus offer one session choice per configured server, disabled while that server
is not connected, plus New Terminal. Sessions and terminals stay available when you close and reopen
ur.

The config file at `$XDG_CONFIG_HOME/ur/config.json` (or `~/.config/ur/config.json`) stores servers
in menu order. Server IDs stay the same when a server is renamed:

```json
{"servers": [{"id": "stable-server-id", "name": "My agent", "command": "/absolute/path/to/agent", "args": []}]}
```

`ur agent-run [--server NAME] <workspace> <prompt>` runs one prompt directly. The name is optional
when exactly one server is configured and required when several are configured.

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
option. The New Session shortcut opens the server choices when several servers are configured. With
one connected server it creates a session directly; with none it opens server settings.
