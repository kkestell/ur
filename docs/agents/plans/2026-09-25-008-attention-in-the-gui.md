# Attention in the GUI

## Goal

Build the Attention in the GUI section of `docs/agents/todo.md`. Today the sidebar lists sessions by
last activity with no status, the GUI never sends `focus`, and a pending permission request is
invisible in the thread: the tool call row stays as it was and only the editor's Stop button hints
that the turn is waiting. When this is done:

- The sidebar puts workspaces with sessions that need attention first, shows a count of those
  sessions at the right of each such workspace, puts sessions that need attention first within their
  workspace, and marks each session's status at the right edge of its row: a dot for
  `NeedsPermission`, a spinner for `Working`, and `!` for `Failed`. Unread sessions are bold.
- The GUI focuses the selected session while its window has focus and nothing otherwise, so viewing
  a session clears its unread flag and a session being viewed does not become unread.
- Each pending permission request of the selected session renders in the thread as in the permission
  wireframe: the tool call title and content the request supplies, merged with the tool call row of
  the same ID, one row per permission option with an icon from its kind, its label, and its
  shortcut, then "Awaiting Confirmation." A request whose tool call has no row yet renders after the
  last entry. Clicking an option or pressing its shortcut answers the request.
- A rejected prompt's error sits directly beneath its user message.

The daemon and the wire protocol do not change. When this is done, the scenario at the top of
`docs/agents/architecture.md` works with the sessions created from the CLI and everything else done
in the GUI.

## Related code

- `crates/ur-client/src/protocol.rs` — `Request::Focus { sessions }`, which replaces the socket
  connection's focus and clears the unread flags;
  `Request::AnswerPermission { session,
  request_id, option_id }`;
  `Status::NeedsPermission { requests }` with `PendingPermission {
  request_id, request }`, where
  `request` is the ACP `RequestPermissionRequest` with `toolCall`, a `ToolCallUpdate`, and
  `options`; `SessionSummary.unread`.
- `crates/ur/src/daemon/state.rs` — `focus()` fails for an unknown session and changes nothing;
  `answer_permission()` fails with "permission request N is resolved" when a request was already
  answered, which is what a second answer gets under first answer wins. Every status change
  publishes `session_changed`, so the watch store always holds the current pending requests.
- `crates/ur/src/cli/answer.rs` — `ur approve` and `ur deny`, the CLI's way of answering the
  session's oldest pending permission request. The shortcuts in the GUI do the same.
- `crates/ur-fake-server/src/lib.rs` — `ask()` sends a `tool_call` with the title "count the
  tallies" and kind `search`, then a permission request whose `toolCall` carries only the ID, with
  options "Go ahead" (`allow_once`) and "Hold off" (`reject_once`). So a request's title and content
  can be absent, and the merged tool call row supplies the title.
- `app/src-tauri/src/link.rs` — `Link` with `Desired { watch, subscribed }` and `run()`, which
  replays the desired set after each connect. Focus joins the desired set here.
- `app/src-tauri/src/main.rs` — the builder. It gains `on_window_event` for `WindowEvent::Focused`,
  which `tauri-runtime` defines as `Focused(bool)`.
- `app/src-tauri/src/commands.rs` — the core commands. `set_visible` is added here.
- `app/src/store/watch.ts` — `WatchState`, `reduceWatch()`, and `workspaceSessions()`, which orders
  a workspace's sessions by last activity. Attention ordering goes here.
- `app/src/components/Sidebar.tsx` — the workspaces and their session rows.
- `app/src/App.tsx` — `Session`, which reads the session's status from the watch store and renders
  `Thread` and `Editor`. It gains the pending requests, `set_visible`, and the shortcuts.
- `app/src/components/Thread.tsx` and `app/src/transcript/blocks.ts` — the blocks and their
  rendering. `tool_call` blocks hold `id`, `title`, `toolKind`, and `status`.
- `app/src/styles.css` — the sidebar rows, the blocks, and the spinner.
- `app/node_modules/@agentclientprotocol/sdk/dist/schema/types.gen.d.ts` — `ToolCallUpdate` with its
  optional `title` and `content`, `ToolCallContent` (`content`, `diff`, or `terminal`),
  `PermissionOption { optionId, name, kind }`, and `PermissionOptionKind`.

## Decisions

### Attention ordering in the watch store

`needsAttention(summary)` is true when the status is `needs_permission` or `failed`, or `unread` is
set, the definition under Session status in `docs/agents/architecture.md`.

- `workspaceSessions()` keeps its last-activity order but puts sessions that need attention first.
  The sort is stable, so within each group the last-activity order holds.
- `orderedWorkspaces(state)` returns the workspaces with at least one session needing attention
  first, in creation order, then the rest in creation order.
- `attentionCount(state, workspace)` counts the workspace's sessions that need attention. The
  sidebar shows it at the right of the workspace name when it is above zero.

The sidebar reorders as statuses change; a session that gains attention moves to the top of its
workspace, which is the wireframe's behavior and what makes the sessions needing attention easy to
find after coming back to the window.

### Status marks and unread in the sidebar

Each session row gets a `status` span at its right edge: a filled dot for `needs_permission`, the
existing `spinner` class at a smaller size for `working`, `!` for `failed`, and nothing for `idle`.
An unread session's row is bold. These are CSS classes on the row, nothing more.

### Focus goes through the core

The webview calls the new core command `set_visible(sessions)` with the selected session as a
one-element list, or an empty list when a terminal or nothing is selected. The core combines that
with the window's focus and sends `focus` to the daemon, as described under GUI architecture in
`docs/agents/architecture.md`.

- `Desired` gains `visible: Vec<String>` and `focused: bool`. `focused` starts `true`, since the
  window has focus when it opens, and `WindowEvent::Focused(focused)` from the builder's
  `on_window_event` updates it.
- `Link::set_visible(sessions)` stores the list, and `Link::set_focused(focused)` stores the flag.
  Both then call `Link::send_focus()`, which sends `Request::Focus` with `visible` when `focused`
  and an empty list otherwise, in a spawned task, when connected. A failed `focus`, such as one
  naming a session the daemon no longer has, is logged and otherwise ignored: the daemon changed
  nothing, and the next selection change sends a fresh one.
- `run()` replays focus after `watch` and the subscriptions, so the daemon knows the sessions before
  it is asked to focus them. On reconnect after a daemon restart, the daemon accepts socket
  connections only after listing every workspace's sessions, so a saved session is there.

The webview sends `set_visible` from an effect on the rendered selection, which is `null` when the
saved session is gone, so the daemon is never asked to focus a session the sidebar does not show.
With panes, `set_visible` will carry every session visible in a pane; nothing changes in the core.

### Pending requests render from the watch store

The pending permission requests live in the session's status in the watch store, not in the thread
state: the daemon sends them with `session_changed`, and they must not survive a session snapshot or
an `entry` in a way the transcript reducer would have to track. `Session` in `App.tsx` reads the
summary's `requests` when the status is `needs_permission` and passes them to `Thread` with the
blocks.

`app/src/transcript/permissions.ts` exports the pure function `withPermissions(blocks, requests)`,
which returns the thread's items:
`Item = Block | { kind: "permission"; request: PendingPermission;
title: string }`. For each
request, oldest first:

- If a `tool_call` block has the request's `toolCall.toolCallId`, the permission item takes that
  block's place, with the request's `toolCall.title` when it is a string and otherwise the block's
  title. No subagent detection or ID parsing.
- Otherwise the permission item is appended after the last item, with the request's title, or the
  tool call ID when the request has no title.

`Thread` renders the items, and `Permission` renders one permission item as in the wireframe: a
block with the title, then the request's `toolCall.content` when it is an array, then one row per
option, then "Awaiting Confirmation." beneath the block. Content items of type `content` with text
content render as preformatted text; a `diff` renders its path and new text as preformatted text;
`terminal` content and non-text content are skipped, since ur advertises no terminal capability and
image content belongs to Session management and editor. Thread rendering refines this later.

Each option row is a button with an icon from its kind, `✓` for `allow_once` and `allow_always` and
`✕` for `reject_once` and `reject_always`, the option's `name`, and its shortcut when it has one.
Clicking it sends `answer_permission` with the session, the request ID, and the option ID. An
`error` response, such as "permission request N is resolved" when another daemon client answered
first, is logged: the daemon's next `session_changed` removes the request from the thread anyway.

### Shortcuts by option kind

The wireframe shows Approve with ⌘Y and Deny with ⌥⌘Z. Each permission option kind has one shortcut:

| Kind            | Shortcut |
| --------------- | -------- |
| `allow_once`    | ⌘Y       |
| `allow_always`  | ⇧⌘Y      |
| `reject_once`   | ⌥⌘Z      |
| `reject_always` | ⇧⌥⌘Z     |

`app/src/keys.ts` exports `shortcutKind(event)`, which maps a keyboard event to a
`PermissionOptionKind` or `undefined`, and `shortcutLabel(kind)`, the string a row shows. A shortcut
answers the selected session's oldest pending request with its first option of that kind, matching
`ur approve` and `ur deny`; when the oldest request has no option of that kind, nothing happens. A
request with two options of the same kind shows the shortcut on the first. `Session` installs one
`keydown` listener on `window` while it is mounted, so the shortcuts work with the editor focused.
Only the four kinds ACP defines exist, so the mapping is a fixed table.

### A rejected prompt's error beneath the user message

The turn error entry of a rejected prompt already follows its user prompt entry, so its `error`
block is already the next block. A CSS adjacency rule, `.block.user + .block.error`, pulls it up
under the user box with no gap, so it reads as part of the prompt. A failed turn's error, which
follows agent output, keeps the ordinary spacing. No reducer change.

## Naming

- Needs attention, unread, focus, visible session, pending permission request, permission option,
  first answer wins, resolved, session summary, watch, thread, editor, sidebar, selection, tool
  call, tool call title, desired set, Link, core, webview — as defined in `docs/agents/glossary.md`.
- `needsAttention()`, `orderedWorkspaces()`, `attentionCount()` — the watch store's attention
  helpers.
- `set_visible` — the core command that reports the visible sessions; `setVisible()` in `ipc/`.
- `Desired { watch, subscribed, visible, focused }` — the desired set in code.
- `Link::set_visible()`, `Link::set_focused()`, `Link::send_focus()` — the core's focus path.
- `withPermissions()` — the pure function that places the pending permission requests among the
  blocks; `Item` — a block or a permission item, what `Thread` renders.
- `Permission` — the component for one pending permission request.
- `shortcutKind()`, `shortcutLabel()` — the shortcut table in `keys.ts`.
- `status` — the class of the mark at the right edge of a session row.

## Test plan

Unit tests under vitest:

- `app/src/store/watch.test.ts`:
  - `sessions_needing_attention_come_first`: within one workspace, a `needs_permission` session, a
    `failed` session, and an unread `idle` session come before a read `idle` session with a newer
    last activity, and keep last-activity order among themselves.
  - `workspaces_needing_attention_come_first`: with three workspaces in creation order and only the
    third having an unread session, `orderedWorkspaces()` returns the third first, then the first
    and second, and `attentionCount()` is 1 for the third and 0 for the others.
- `app/src/transcript/permissions.test.ts`:
  - `a_request_takes_its_tool_call_rows_place`: a request for an existing tool call ID replaces that
    block at the same position, with the block's title when the request has none and the request's
    title when it has one.
  - `a_request_without_a_tool_call_row_follows_the_last_entry`: a request for an unknown ID is
    appended after the blocks, titled by its tool call ID.
  - `requests_keep_their_order`: two requests, one merged and one appended, leave the other blocks
    in place and the appended one last.
- `app/src/keys.test.ts`: `shortcuts_map_to_option_kinds`, table-driven over the four shortcuts and
  one unrelated key.

No Rust test changes: the daemon and the wire protocol are unchanged, and the core's focus path is
thin.

End-to-end tests in `app/e2e/attention.test.ts`, against the fake server's scripts:

- `a working session shows the spinner`: `hold`.
- `a session waiting for permission shows its mark and its workspace's count`: `tool`.
- `clicking a permission option answers the request`: the request's title and "Awaiting
  Confirmation." render, and clicking Go ahead completes the turn with `selected go` and clears the
  mark.
- `the permission shortcuts answer the oldest request`: `tools` asks twice; ⌘Y answers the first
  with `go` and ⌥⌘Z the second with `stop`.
- `a session that finishes a turn while not shown is unread until it is shown`: selecting it clears
  the bold.
- `a failed turn shows its error and the failed mark`: `fail`.
- `a rejected prompt shows its error under the user message`: `reject`.

The GUI focuses its visible sessions only while its window has focus, and WebDriver cannot focus the
window. `Gui.focusWindow()` makes `ur-app` the frontmost application through System Events;
`openGui()` calls it, and the unread test calls it again before selecting the session.
`Gui.pressShortcut()` presses ⌘ with a key, and ⇧ or ⌥, on the window.

Run the end-to-end suite at the end, since this finishes a section of `docs/agents/todo.md`. Run the
check in `docs/agents/todo.md` with Ox by hand: create sessions from the CLI in two workspaces, then
in the GUI prompt one with a command that needs permission and confirm the dot in the sidebar, the
workspace count, the request in the thread, and that ⌘Y answers it; confirm the spinner while a turn
runs, that a session prompted from the CLI while another is selected becomes bold, and that
selecting it clears the bold; confirm a `!` and its error for a failed turn; and confirm a rejected
prompt's error under its user message.

## Implementation plan

Sidebar:

1. `app/src/store/watch.ts`: `needsAttention()`, attention-first `workspaceSessions()`,
   `orderedWorkspaces()`, and `attentionCount()`, with their tests.
2. `app/src/components/Sidebar.tsx` and `app/src/styles.css`: the workspace order and counts, the
   session order, the `status` mark, and bold unread rows.

Focus:

3. `app/src-tauri/src/link.rs`: `visible` and `focused` in `Desired`, `set_visible()`,
   `set_focused()`, `send_focus()`, and the replay in `run()`. `app/src-tauri/src/commands.rs`:
   `set_visible`. `app/src-tauri/src/main.rs`: register it and add `on_window_event` for
   `WindowEvent::Focused`.
4. `app/src/ipc/index.ts`: `setVisible()`. `app/src/App.tsx`: the effect that sends it for the
   rendered selection.

Permission requests:

5. `app/src/transcript/permissions.ts` with `withPermissions()` and `Item`, and its tests.
6. `app/src/keys.ts` with `shortcutKind()` and `shortcutLabel()`, and its test.
7. `app/src/components/Permission.tsx`; `Thread.tsx` renders items; `App.tsx`'s `Session` passes the
   pending requests, installs the `keydown` listener, and answers through `answer_permission`.
   `styles.css`: the permission block, option rows, and "Awaiting Confirmation."
8. `app/src/styles.css`: the `.block.user + .block.error` rule.
9. `app/e2e/harness.ts`: `Gui.focusWindow()`, called from `openGui()`, and `Gui.pressShortcut()`.
   `app/e2e/attention.test.ts`: the tests in the Test plan.
10. Run the end-to-end suite and the check in `docs/agents/todo.md` with Ox.

## Documentation updates

- `docs/agents/testing.md`: the attention guarantees, and `focusWindow()`.
- `AGENTS.md`: map `app/e2e/attention.test.ts`, `app/src/transcript/permissions.ts`,
  `app/src/keys.ts`, and `app/src/components/Permission.tsx`; add `set_visible` to `commands.rs` and
  `setVisible()` to `ipc/`; say `link.rs`'s desired set includes the visible sessions and the window
  focus; add `orderedWorkspaces()` and `attentionCount()` to `store/watch.ts`.
- `docs/agents/architecture.md`: under GUI architecture, say the desired set now includes focus,
  that the core sends `focus` on each `set_visible` and window focus change, and that pending
  permission requests render from the watch store, placed among the blocks by
  `transcript/permissions.ts`. Under Permissions, say the GUI's shortcuts answer the selected
  session's oldest request by option kind, like `ur approve` and `ur deny`.
- `docs/agents/glossary.md`: under Desired set, list focus as restored; under Visible session, say
  it is the selected session until Pane grid; add Shortcut under Permissions: the key combination
  for a permission option kind, which answers the selected session's oldest pending request.
