# Session management and editor

## Goal

Build the Session management and editor section of `docs/agents/todo.md`. Today workspaces and
sessions can only be created from the CLI, sessions cannot be deleted, and the editor is a plain
textarea with Send and Stop. When this is done:

- The sidebar header's `+` opens a folder picker and adds a workspace named after the folder.
  Right-clicking a workspace opens the workspace menu: New Session and Remove Workspace…, which asks
  for confirmation first.
- A session header above the thread shows the session title and a `+` that creates a session in the
  same workspace and selects it.
- When the server advertises `session/delete`, right-clicking a session opens the session menu with
  Delete…, which asks for confirmation, cancels a running turn, waits for it to end, and calls
  `session/delete`. The session leaves the sidebar.
- Typing `/` in the editor lists the slash commands from the latest `available_commands_update`.
- The editor's bottom row shows the usage indicator when the server reported usage, one config
  picker per config option, then Send or Stop. Choosing a value sends `set_config_option`.
- When the server supports image prompts, dropping an image file on the editor attaches it to the
  next prompt as a chip with ×. The prompt sends it as ACP image content. A server rejection is an
  ordinary rejected prompt.

Ox advertises none of delete, config options, usage, or image prompts, so the fake server gains them
for the tests and the ad-hoc check.

## Related code

- `crates/ur-client/src/protocol.rs` — `Request`, `Event`. Gains the requests and events below.
- `crates/ur/src/daemon/state.rs` — `State`, `Session`, `Op`, `Server`. Config options, delete, and
  capabilities go here.
- `crates/ur/src/daemon/ops.rs` — `new_session()` is the pattern for a request answered from an
  `on_receiving_result` callback; `load()` and `send_prompt()` build the callbacks that
  `finish_load()` and `finish_prompt()` end.
- `crates/ur/src/daemon/server.rs` — `handle()`, which dispatches requests.
- `crates/ur/src/daemon/tests.rs` — `TestDaemon`, `Subscription`, and `Watch`. The helpers panic on
  unexpected events, so they learn the new ones.
- `crates/ur/src/cli/mod.rs` and `cli/read.rs` — destructure `WatchSnapshot` and `SessionSnapshot`.
- `crates/ur-fake-server/src/lib.rs` — `fake_server()`, `SavedSession`, and the scripts.
- `app/src-tauri/src/link.rs` — `run()` routes each event to the `watch` or `session` name.
- `app/src-tauri/src/main.rs`, `capabilities/default.json`, `tauri.conf.json` — plugins,
  permissions, and the window config.
- `app/src/ipc/index.ts` — `WatchEvent` and `SessionEvent`.
- `app/src/store/watch.ts` and `app/src/transcript/reduce.ts` — the reducers the new events join.
- `app/src/components/Sidebar.tsx`, `Editor.tsx`, and `app/src/App.tsx` — the controls.
- `docs/agents/wireframes/menus.png`, `editor.png`, and `main-window.png` — the controls to match.
- `agent-client-protocol-schema` 1.7.0, `src/v1/agent.rs` — `DeleteSessionRequest`,
  `SetSessionConfigOptionRequest` and its `SessionConfigOptionValue`, `SessionConfigOption`, and
  `config_options` on `NewSessionResponse` and `LoadSessionResponse`; `SessionCapabilities.delete`
  and `PromptCapabilities.image`. None needs an unstable feature.

## Decisions

### Wire protocol

New requests:

```rust
/// Deletes a session from the server, when it advertises `session/delete`.
/// A running turn is cancelled first, and the response waits for
/// `session/delete` to return.
DeleteSession { session: SessionId },
/// Sends `session/set_config_option` and answers when the server responds.
SetConfigOption {
    session: SessionId,
    config_id: SessionConfigId,        // ts: string
    value: SessionConfigOptionValue,   // ts: { type: "boolean"; value: boolean } | { value: string }
},
```

`config_id` follows ACP's `configId`; `option_id` already means a permission option.

Event changes:

- `WatchSnapshot` gains `capabilities: Option<AgentCapabilities>`, the current ACP connection's
  capabilities, or `None` without one.
- `CapabilitiesChanged { capabilities: AgentCapabilities }`, a watch event, is broadcast by
  `set_server` on each successful `initialize`, so a GUI watching since before a server restart, or
  since before the first `initialize` succeeded, learns them.
- `SessionDeleted { session }`, a watch event: the session is gone from the sidebar.
- `SessionSnapshot` gains `config_options: Vec<SessionConfigOption>`.
- `ConfigOptionsChanged { session, config_options }`, a subscribe event after the snapshot.
- `SessionRemoved` now means removed with its workspace or deleted. A deleted session sends it to
  subscribers and `SessionDeleted` to watchers, so the Link keeps routing each event type to one
  name: `CapabilitiesChanged` and `SessionDeleted` to `watch`, `ConfigOptionsChanged` to `session`.

ACP types go through unchanged, with `#[ts(type = ...)]` overrides to the SDK types as elsewhere in
`protocol.rs`.

### Config options live on the daemon's session

`Session` gains `config_options: Vec<SessionConfigOption>`, set from:

- the `session/new` response, passed to `add_session`;
- the `session/load` response, when it has any: `finish_load` takes
  `Result<Option<Vec<SessionConfigOption>>, String>`;
- the `session/set_config_option` response, through a new `State::set_config_options`;
- `config_option_update`, in `apply_update`, live or replayed.

Every change outside a load sends `ConfigOptionsChanged` to subscribers. During a load, nothing is
sent; the snapshot at the end of the load carries them. The `config_option_update` entry also stays
in the transcript as an ACP update entry, like every other update; the transcript reducer ignores it
and reads config options only from the snapshot and `ConfigOptionsChanged`.

`set_config_option` takes no operation guard. It is sent through the ACP connection and answered
from its callback, like `new_session`: `Done` with the new config options applied, or
`Error { "session/set_config_option failed: …" }`. An unloaded session is not loaded first; the
server's error is shown.

### Slash commands and usage come from the transcript

`available_commands_update` and `usage_update` are already ACP update entries, so the daemon does
not change for them. The transcript reducer keeps the latest of each in `ThreadState`, from the
snapshot and later entries alike. A slash command is sent as ordinary prompt text, which is what ACP
specifies.

### Delete holds the operation guard

`Op::Prompt` becomes `Op::Prompt { delete: Option<(u64, Outbox)> }`, and `Op` gains `Delete`.
`Server::can_delete()` reads `session_capabilities.delete`.

`State::delete_session(id, request_id, outbox, send_cancel, send_delete)`:

- The server does not advertise delete: error "the server cannot delete sessions".
- `op` is `None`: send `session/delete`, set `Op::Delete`, and answer later.
- `op` is `Prompt { delete: None }`: cancel as `cancel()` does, record `(request_id, outbox)` in
  `delete`, and answer later.
- `op` is `Load`, `Delete`, or `Prompt { delete: Some(_) }`: busy.

`finish_prompt` returns the waiting delete, if any. The `send_prompt` callback then calls
`State::start_delete` under the same lock, so no other request takes the guard in between; if
sending fails, the waiting daemon client gets the error.

`send_delete(state, request_id, outbox)` in `ops.rs` builds the function that sends
`session/delete`. Its callback answers the daemon client in every case, `Done` or
`Error { "session/delete failed: …" }`, and calls `State::finish_delete(id, generation, result)`.
`finish_delete` ignores an earlier generation or a missing session. On success it removes the
session, sends `SessionRemoved` to its subscribers, and broadcasts `SessionDeleted`. On failure it
releases the guard and the session stays.

`server_exited` answers a waiting delete with "the server exited before the session was deleted".
`remove_workspace` answers a waiting delete `Done`, since the session is gone.

### Capabilities reach the GUI through watch

The GUI needs two capabilities: `sessionCapabilities.delete` for the session menu, and
`promptCapabilities.image` for image drops. `WatchState` gains
`capabilities: AgentCapabilities |
null`, from the snapshot and `CapabilitiesChanged`.

### Menus and dialogs are built in the webview

The workspace and session menus use `Menu` from `@tauri-apps/api/menu` and `menu.popup()`, which
shows the same native menu `Menu::popup` does, and needs only `core:default`. The folder picker,
confirmations, and error messages use `open`, `ask`, and `message` from `@tauri-apps/plugin-dialog`.
The core gains the dialog plugin and no commands. These live in `app/src/actions.ts`:

- `addWorkspace()`: `open({ directory: true })`; nothing when cancelled; otherwise `add_workspace`
  with the folder's last path component as the name. An error response, such as a duplicate name,
  shows through `message(…, { kind: "error" })`.
- `newSession(workspace, onSelect)`: `new_session`, then selects the created session. Errors show
  through `message`.
- `removeWorkspace(watch, name)`: `ask` titled "Remove workspace?" with `Remove "<name>"?`, adding
  "Its running turns will be cancelled." when one of its sessions is `working` or
  `needs_permission`, and an okLabel of "Remove"; then `remove_workspace`.
- `deleteSession(summary)`: `ask` titled "Delete session?" with
  `Delete "<session title>" and its saved transcript?`, adding "Its running turn will be cancelled."
  when the session is `working` or `needs_permission`, and an okLabel of "Delete"; then
  `delete_session`.
- `showWorkspaceMenu(watch, workspace, onSelect)`: New Session, a separator, and Remove Workspace….
- `showSessionMenu(summary)`: Delete…. The session row calls it only when the server advertises
  delete.

The "No workspaces" empty state replaces its CLI hint with an Add Workspace button that calls
`addWorkspace()`.

### Images are read in the webview

`tauri.conf.json` sets `"dragDropEnabled": false` on the window, so the webview receives ordinary
HTML drop events with `File` objects instead of Tauri's drag-drop event. `main.tsx` calls
`preventDefault()` on `dragover` and `drop` on `window`, so a file dropped outside the editor does
not navigate the webview to it.

The editor handles `dragover` and `drop`. On drop, each file whose `type` starts with `image/` is
read with `FileReader.readAsDataURL`, and the base64 after the comma becomes an `ImageAttachment`
`{ name, mimeType, data }`. A non-image file shows "<name> is not an image." in the editor message
line. Without `promptCapabilities.image`, a drop shows "The server does not accept images." and
attaches nothing.

Attachments show as chips above the textarea with the image glyph, the file name, and ×. Send is
allowed with text, attachments, or both. The prompt's content is the text block, when there is text,
followed by one `{ type: "image", mimeType, data }` block per attachment. The attachments clear on
send and come back with the text when the prompt is busy or answers an error, as the text does
today. No size or type limits beyond `image/`: the server's rejection is an ordinary rejected
prompt.

The user message shows its images: the user block gains
`images: { mimeType: string; data: string
}[]`, from the image parts of a user prompt entry and of
replayed `user_message_chunk` updates, drawn as thumbnails under the text through `data:` URLs.

### Editor

`Editor` takes the thread and the capabilities besides the session and status.

- Command list: when the text starts with `/` and has no whitespace, and the session has slash
  commands, a list above the editor shows each command whose name starts with the typed text, with
  its name in the buffer font and its description. ↑ and ↓ move the highlight, Enter or Tab replaces
  the text with `/<name>`, and Escape closes the list. With no matching command, Enter sends as
  usual. The placeholder is "Message agent — / for commands" when the session has slash commands,
  and "Message agent" otherwise. `slashQuery(text)` and `matchingCommands(commands, query)` in
  `app/src/slash.ts` hold the pure part.
- Usage indicator, `app/src/components/UsageIndicator.tsx`: an SVG ring filled by `used / size`.
  Hovering shows a popover with "84k / 200k tokens (42%)" and, when `cost` is present, "Cost $1.27",
  formatted by `Intl.NumberFormat` with the supplied currency. `usageText(usage)` in
  `app/src/usage.ts` returns both lines.
- Config picker, `app/src/components/ConfigPicker.tsx`, one per config option in the supplied order:
  - `select`: a button with the current value's name and ⌄. Clicking opens a list above it with each
    value's name, its description in the dim colour, and ✓ on the current value. Grouped values show
    each group's name as a header. With more than 8 values, a filter field at the top narrows the
    list by name. Choosing a value sends `set_config_option` with `{ value }` and closes the list.
  - `boolean`: a toggle button with the option name, highlighted when on, that sends
    `{ type: "boolean", value }`.
  - An error response shows in the editor message line.
- The bottom row is the usage indicator, the config pickers, then Send or Stop.

### Session header

`Session` in `App.tsx` renders a header above the thread: the session title, or "New session", and a
`+` that calls `newSession(summary.workspace, onSelect)`.

### Fake server

- Advertises `promptCapabilities.image` always, and `sessionCapabilities.delete` when history is
  advertised.
- `SavedSession` gains `pace: String`, "steady" at first. Every `session/new` and `session/load`
  response carries one select config option: ID `pace`, name "Pace", values `steady` ("Steady") and
  `brisk` ("Brisk").
- `session/set_config_option` sets `pace` and answers the config options; an unknown config ID or
  value answers invalid params.
- `session/delete` removes the saved session and its loaded mark.
- `session/prompt` accepts any mix of text and image blocks. The script comes from the text blocks
  joined, and the reply ends with `(N images)` when N is more than zero.
- New scripts: `pace` sends a `config_option_update` setting `pace` to `brisk`, and `usage` sends a
  `usage_update` with `used` 1200, `size` 8000, and a cost of 0.25 USD.

### Dependencies

`tauri-plugin-dialog` joins the workspace dependencies at the version matching `tauri` 2.11 and
`app/src-tauri/Cargo.toml`; `@tauri-apps/plugin-dialog` joins `app/package.json`. The main window
gains `dialog:allow-open`, `dialog:allow-ask`, and `dialog:allow-message`.

## Naming

- Workspace, session, session title, config option, slash command, usage indicator, image
  attachment, capabilities, operation guard, busy, cancellation, subscribe, watch, session snapshot,
  sidebar, editor, thread — as defined in `docs/agents/glossary.md`.
- Workspace menu — the native context menu of a workspace row: New Session and Remove Workspace….
- Session menu — the native context menu of a session row: Delete….
- Session header — the row above the thread with the session title and `+`.
- Command list — the list of matching slash commands above the editor while typing `/`.
- Config picker — the editor control for one config option. `ConfigPicker` in code.
- `UsageIndicator` — the usage indicator component.
- `ImageAttachment` — an image attachment in the editor before it is sent:
  `{ name, mimeType, data }`.
- Waiting delete — the `delete_session` request recorded in `Op::Prompt { delete }` until the
  cancelled turn ends.
- `delete_session`, `set_config_option`, `capabilities_changed`, `session_deleted`,
  `config_options_changed` — the new requests and events.

## Test plan

Daemon tests in `crates/ur/src/daemon/tests.rs`, against the fake server:

- `deleting_a_session_removes_it`: subscribe to a session and watch, delete it: `Done`, the watcher
  gets `SessionDeleted`, the subscriber `SessionRemoved`, and a second daemon on the same saved
  history does not list it. Against `SavedHistory::unadvertised()`, delete answers the error.
- `deleting_a_running_session_cancels_its_turn_first`: prompt `tool` and wait for `NeedsPermission`,
  send delete without awaiting it, and a second delete is busy. The watcher sees `Working`, then
  `Idle { Cancelled }`, then `SessionDeleted`, and the first delete answers `Done`.
- `set_config_option_changes_the_config_options`: the snapshot holds `pace` at `steady`; setting
  `brisk` answers `Done` and the subscriber gets `ConfigOptionsChanged` with `brisk`; setting an
  unknown value answers an error and sends nothing.
- `config_option_updates_change_the_config_options`: prompting `pace` sends `ConfigOptionsChanged`
  with `brisk`.
- `a_load_restores_the_config_options`: set `brisk`, restart the server; the reloaded session's
  snapshot holds `brisk`, from the load response.
- `watch_reports_the_capabilities`: the watch snapshot's capabilities advertise delete and image
  prompts, and after the server restarts the watcher gets `CapabilitiesChanged`.

The `Watch` and `Subscription` helpers record capabilities and config options instead of panicking
on the new events, and existing tests matching the snapshots gain the new fields.

Webview unit tests under vitest:

- `reduce.test.ts`: a table-driven case each for the latest `available_commands_update` replacing
  earlier commands, the latest `usage_update`, config options from the snapshot and from
  `config_options_changed`, and images in a user block from a user prompt entry and from replayed
  `user_message_chunk` updates.
- `watch.test.ts`: `session_deleted` removes the session; the snapshot and `capabilities_changed`
  set the capabilities.
- `slash.test.ts`: table-driven `slashQuery` (`/`, `/ta`, `/tally x`, `hi /x`) and
  `matchingCommands`.
- `usage.test.ts`: table-driven `usageText`, with and without cost, below and above 1000 tokens.

End-to-end tests against the fake server. WebDriver cannot drive the native folder picker, menus, or
dialogs, so a test that needs one of them does its setup through the command line or `Gui.request()`
and checks the GUI's side.

- `app/e2e/editor.test.ts`:
  - `typing / lists the server's slash commands, and Enter inserts one`: `/tally` and its
    description; Enter leaves `/tally` in the editor.
  - `choosing a config option value sets it on the server`: Brisk in the Pace picker, still Brisk
    after reopening the GUI.
  - `a config option the server changes updates its picker`: the `pace` script.
  - `usage the server reports shows in the usage indicator`: the `usage` script; hovering the ring
    shows `1.2k / 8k tokens (15%)` and `Cost $0.25`.
  - `an image dropped on the editor is sent with the prompt`: a PNG's chip, the reply ending
    `(1 image)`, and the user message's thumbnail.
  - `a dropped file that is not an image is refused`.
- `app/e2e/session.test.ts`:
  - `the session header shows the session title`: New session, then the `title` script's title in
    the header and the sidebar.
  - `the session header's + creates a session in its workspace and selects it`.
  - `deleting a session removes it from the sidebar`: through `Gui.request()`.

The harness gains `Gui.hover()`, which sends a `mouseover` that React reports as `onMouseEnter`,
`Gui.dropFile()`, which drops a `File` through a `DataTransfer`, and `Gui.pressKey()`.

Run the end-to-end suite at the end, since this finishes a section of `docs/agents/todo.md`. Then:

- Ad-hoc check against the fake server: add a workspace with `+`, create sessions from the workspace
  menu and the session header, pick Brisk in the Pace picker, type `/` and insert `/tally`, send
  `usage` and hover the ring, drop a PNG and send it (the reply ends with `(1 image)`), delete a
  session holding a `tool` permission request, and remove the workspace.
- The check in `docs/agents/todo.md` with Ox, entirely in the GUI.

## Implementation plan

1. `protocol.rs`: the requests and event changes above.
2. `state.rs`: `config_options` on `Session`, `set_config_options`, `apply_update` and `finish_load`
   changes, `ConfigOptionsChanged`; the watch snapshot's capabilities and `CapabilitiesChanged`;
   `Op::Prompt { delete }`, `Op::Delete`, `can_delete`, `delete_session`, `start_delete`,
   `finish_delete`, and the `finish_prompt`, `server_exited`, and `remove_workspace` changes.
3. `ops.rs` and `server.rs`: `delete_session()`, `send_delete()`, `set_config_option()`, the
   `new_session`, `load`, and `send_prompt` callback changes, and dispatch in `handle()`.
4. `cli/mod.rs` and `cli/read.rs`: compile against the new fields.
5. Fake server: capabilities, `pace`, `session/set_config_option`, `session/delete`, image prompts,
   and the `pace` and `usage` scripts.
6. Daemon tests and helpers.
7. `link.rs`: route the new events. Regenerate the bindings with `cargo test`.
8. The dialog plugin: workspace dependency, `app/src-tauri/Cargo.toml`, `main.rs`,
   `capabilities/default.json`, and `pnpm -C app add @tauri-apps/plugin-dialog`.
9. `ipc/index.ts`, `store/watch.ts`, `transcript/blocks.ts`, `transcript/reduce.ts`, and
   `store/sessions.ts`, with their tests.
10. `app/src/actions.ts`, `Sidebar.tsx`, and the session header and empty state in `App.tsx`.
11. `slash.ts`, `usage.ts`, `UsageIndicator.tsx`, `ConfigPicker.tsx`, and `Editor.tsx`, with their
    tests; the user message thumbnails in `Thread.tsx`.
12. `tauri.conf.json` `dragDropEnabled`, the `window` handlers in `main.tsx`, and image drops in
    `Editor.tsx`.
13. `styles.css`: the header `+`, session header, command list, chips, thumbnails, ring and popover,
    and pickers, matching the wireframes.
14. `app/e2e/harness.ts`: `Gui.hover()`, `dropFile()`, and `pressKey()`. `app/e2e/editor.test.ts`
    and `session.test.ts`: the tests in the Test plan.
15. Run the end-to-end suite, the ad-hoc check, and the check with Ox.

## Documentation updates

- `docs/agents/testing.md`: the session management and editor guarantees.
- `AGENTS.md`: map `app/e2e/editor.test.ts`, `app/src/actions.ts`, `slash.ts`, `usage.ts`,
  `components/ConfigPicker.tsx`, and `components/UsageIndicator.tsx`; update the entries for
  `protocol.rs` (the new requests and events), `state.rs` (config options and deletes), `ops.rs`
  (`delete_session()`, `set_config_option()`), `ur-fake-server`, `main.rs` in the core (the dialog
  plugin), `capabilities/default.json` (dialog permissions), `link.rs`, `store/watch.ts`
  (capabilities), `transcript/blocks.ts` (commands, usage, config options, user images),
  `Sidebar.tsx` (the `+` and menus), `Editor.tsx` (the command list, image attachments, pickers, and
  usage indicator), and `App.tsx` (the session header).
- `docs/agents/architecture.md`: in the wire protocol request list, `delete_session(session)` and
  `set_config_option(session, config_id, value)`; the watch level carries capabilities with
  `capabilities_changed`, and `session_deleted`; the subscribe level's `config_options_changed`;
  under Daemon architecture, `Op::Delete` and the waiting delete in `Op::Prompt`; under GUI
  architecture, native pieces are the dialog plugin, the opener plugin, and the menu API from the
  webview, and image files are dropped on the editor and read by the webview; drop `read_attachment`
  and `menu.rs` from the command list and project layout, and add `actions.ts`, `slash.ts`,
  `usage.ts`, `ConfigPicker`, and `UsageIndicator`.
- `docs/agents/glossary.md`: define workspace menu, session menu, session header, command list,
  config picker, and waiting delete; Image attachment becomes "an image file dropped on the editor,
  read by the webview, and sent as image content in the next prompt"; Watch and Session snapshot
  gain capabilities and config options; the fake server's scripts gain `pace` and `usage`.
- `docs/agents/todo.md`: the image bullet says the webview reads the dropped file; check off Session
  management and editor.
