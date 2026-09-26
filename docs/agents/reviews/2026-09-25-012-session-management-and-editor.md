# Review of session management and editor

## Scope and coverage

Commit `6bc0479`, reviewed against the plan
`docs/agents/plans/2026-09-25-011-session-management-and-editor.md` and its work log. The review
covered the daemon's `delete_session` and `set_config_option` paths in `state.rs`, `ops.rs`, and
`server.rs`, including the waiting delete through `finish_prompt`, `server_exited`, and
`remove_workspace`; the new wire protocol events and their routing in `link.rs`; the CLI changes;
the fake server; the daemon tests; and the webview's `actions.ts`, `Sidebar.tsx`, `App.tsx`,
`Editor.tsx`, `ConfigPicker.tsx`, `UsageIndicator.tsx`, `slash.ts`, `usage.ts`, the watch store, and
the transcript reducer. It also covered how image content reaches the socket frames, in
`crates/ur-client/src/frame.rs` and `client.rs`.

Lenses: correctness, concurrency, error-handling, testing, simplicity, and documentation.

Gaps: `styles.css` was not reviewed against the wireframes, and the GUI was not run.

## Findings

### High

#### Correctness

- **OX-0008 A session holding about 12 MB of images disconnects the GUI in a loop**
  (`crates/ur-client/src/frame.rs:13`, `crates/ur/src/daemon/state.rs:899`): Each user prompt entry
  keeps its image content as base64, and the session snapshot sends the whole transcript in one JSON
  frame. `FrameCodec` encodes a frame of any size, but its decoder rejects a payload over
  `MAX_PAYLOAD_LEN`, 16 MiB, which is about 12.5 MB of image data before base64. When a subscribed
  session holds more than that, for example four or five full-screen Retina screenshots, the core's
  reader in `Client` (`client.rs:54`) stops at the decode error and the socket connection closes.
  The Link reconnects and replays `subscribe` for that session. The daemon sends the same oversized
  snapshot, and the connection closes again. The whole GUI keeps showing "Connecting" until the
  daemon restarts. The saved selection subscribes to the session again at startup, and after a
  daemon restart a load can rebuild the same transcript from replayed `user_message_chunk` images. A
  single prompt with one image over about 12.5 MB fails the same way: the core sends a request frame
  the daemon rejects, and the daemon closes the socket connection. Possible fixes: raise
  `MAX_PAYLOAD_LEN` well above realistic transcripts; limit the size of an image attachment in the
  editor and report the limit in the editor message line; or keep image data out of the session
  snapshot and give it its own transport.

## Checks run

- `make check`: formatting, the Rust tests (33 daemon tests and the rest), the build, clippy,
  `pnpm -C app build`, and the 46 vitest tests passed. `dprint check` failed only on this review
  before `make format-docs`, and passes after it.
- Traced the frame path: `FrameCodec::encode` writes any length, `FrameCodec::decode` rejects over
  16 MiB, and `Client`'s reader ends on the first decode error, which closes the events receiver the
  Link loops on.

## Verdict

The delete, config option, capability, and editor changes match the plan, and the delete and cancel
interleavings are handled and tested. One high severity issue is left open: images accumulated in
one session can put the GUI in a reconnect loop. Plan a fix before relying on image drops.

Session management and editor stays unchecked in `todo.md`: the work log leaves it open until its
check with Ox has been run.
