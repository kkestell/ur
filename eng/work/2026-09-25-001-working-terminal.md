# Working terminal

## Plan

`eng/plans/2026-09-25-001-working-terminal.md`

## Summary

Milestone 0 is built: the daemon runs login shells through `portable-pty`,
feeds their output through `vt100::Parser`, and restores the screen on
attachment. The GUI shows the selected terminal in xterm.js and reattaches it
after a restart. The plan's goal is met: the milestone 0 check passes with
`top` and vim holding an unsaved buffer, including reopening at a smaller and
a larger window size.

## Departures from the plan

- All three tasks were done in one pass instead of one reviewed slice at a
  time. The final state matches the plan after task 3.
- Attaching a terminal again from the same socket connection replaces the
  earlier attachment (`Outbox::same_connection`). Without it, reloading the
  webview attaches again through the core's one connection, and the terminal's
  output arrives twice.
- `ClientMessage` and `DaemonMessage` derive serde only, not ts-rs. The webview
  never sees them, and their `u64` request ID would become `bigint`.
- The core's `attach_terminal` opens a new terminal after any failed
  attachment, not only a rejection. An I/O error fails the open that follows
  with the same error.
- The webview does not use React `StrictMode`. Its doubled effects in
  development attach the terminal twice.

## Decisions

- `Terminals::attach` holds the terminal map's lock while it registers the
  outbox, and the reader thread removes the terminal from the map before it
  takes the outboxes. An attachment therefore either receives
  `terminal_exited` or fails with an unknown terminal.
- A size with 0 rows or 0 columns is rejected with an error, so bad input
  never reaches `vt100`'s `set_size` under the terminal's mutex.
- No working directory is set on the `CommandBuilder`: portable-pty starts in
  `$HOME` when none is set (`cmdbuilder.rs`, `as_command`).
- A `JSON` frame that is not a valid `ClientMessage` closes that socket
  connection, the same as a malformed frame.
- `TerminalId` is a type alias for `u32`, so the bindings use `number`.
- The daemon uses `anyhow` for errors and `eprintln!` for its log.
- The scaffold's opener plugin, `lib.rs`, README, editor settings, sample
  assets, and Windows Store icons were removed. The app crate is `ur-app`.
- `cargo build --workspace` does not need `app/dist` in a clean checkout, so
  `AGENTS.md` lists `pnpm -C app build` after the Cargo checks.

## Automated checks

- `cargo fmt --all -- --check` — passed.
- `cargo test --workspace --all-targets --all-features` — passed: 2 frame
  tests, `snapshot_reproduces_the_screen`, 5 integration tests, 3 binding
  exports.
- `cargo test -p ur --test terminal`, run 5 more times — passed each time.
- `cargo build --workspace --all-features` — passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` —
  passed.
- `pnpm -C app build` — passed.

## Manual verification

The GUI was checked through an embedded WebDriver server, because screen
capture and accessibility access were unavailable. The `webdriver` feature in
`app/src-tauri` registers `tauri-plugin-wdio-webdriver`. WebDriver key actions
do not work with xterm.js (characters are doubled and read as function keys),
so typing dispatches an `insertText` input event on xterm.js's hidden
textarea, which reaches `onData` like real typing. Screenshots come from the
webview itself.

Setup, with a scratch socket and state directory:

```sh
D=$(mktemp -d)
cargo build -p ur
UR_SOCKET=$D/ur.sock target/debug/ur daemon &
(cd app && UR_SOCKET=$D/ur.sock XDG_STATE_HOME=$D/state \
  TAURI_WEBDRIVER_PORT=4445 pnpm tauri dev --features webdriver) &

W=http://127.0.0.1:4445; H='Content-Type: application/json'
until curl -s $W/status >/dev/null; do sleep 1; done
S=$(curl -s -X POST -H "$H" -d '{"capabilities":{}}' $W/session |
  python3 -c 'import json,sys; print(json.load(sys.stdin)["value"]["sessionId"])')
# The argument is a JSON string body: '\r' is Enter, '\u001b' is Escape.
type() {
  curl -s -X POST -H "$H" $W/session/$S/execute/sync -d "{\"args\":[\"$1\"], \"script\":\"const t=document.querySelector('.xterm-helper-textarea'); t.dispatchEvent(new InputEvent('input',{data:arguments[0], inputType:'insertText',bubbles:true}))\"}" >/dev/null; }
shot() { curl -s $W/session/$S/screenshot |
  python3 -c 'import base64,json,sys; sys.stdout.buffer.write(base64.b64decode(json.load(sys.stdin)["value"]))' > "$1"; }
```

To close and reopen the GUI at another size, stop `pnpm tauri dev`, start it
again with `--config '{"app":{"windows":[{"title":"ur","width":700,"height":450}]}}'`,
and create a new session.

1. The GUI shows a shell and accepts input.

   ```sh
   type 'echo typed-by-webdriver\r'; shot shell.png
   ```

   The command and its output render under the zsh prompt.

2. `top` survives closing and reopening the GUI at a smaller size, and
   quitting it returns to a clean prompt.

   ```sh
   type 'top\r'; shot top-before.png
   # close, reopen at 700x450, new session
   shot top-after.png; type 'q'; shot top-quit.png
   ```

   After reopening, `top` was still running (its clock advanced from 10:13:12
   to 10:13:33) and had redrawn at the smaller size. After `q`, only a fresh
   prompt remained, with no leftover `top` frame.

3. vim keeps an unsaved buffer across closing and reopening the GUI at a
   larger size, and accepts input afterwards.

   ```sh
   type 'vim /tmp/notes.txt\r'
   type 'iline one of the unsaved buffer\rline two, typed through the GUI\u001b'
   # close, reopen at 1200x800, new session
   shot vim-after.png; type 'oline three, after reopening\u001b'
   ls /tmp/notes.txt
   ```

   Both lines were restored and vim redrew at the larger size. The third line
   appeared, and `notes.txt` did not exist on disk.

4. Reopening reattaches the selected terminal instead of opening a new one.

   ```sh
   cat $D/state/ur/gui.json
   ```

   The selection stayed `{"terminal": 1}` for the socket across every
   reopen.

## Follow-up work

- The `webdriver` feature and `tauri-plugin-wdio-webdriver` dependency in
  `Cargo.toml`, `app/src-tauri/Cargo.toml`, and `app/src-tauri/src/main.rs`
  were added for verification after the implementation. They are uncommitted
  and not part of this change.
- The core writes `[shell exited]` when the daemon connection drops, not only
  when the shell exits. Milestone 5's reconnect replaces this.
- The work is not committed yet.
