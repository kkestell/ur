# End-to-end suite

## Goal

Milestone checks need the real GUI: the webview, the core, and the daemon
working together. Today they are run by hand. Add an end-to-end suite that
drives the built app through WebDriver, run with `pnpm -C app e2e` at the end of
each milestone. Keep it small: a closed list of guarantees in `eng/testing.md`,
each owned by one test, covering the most important and trickiest behavior.
Everything else, including one-off checks after a milestone, uses the same
harness from throwaway scripts that are never committed.

When this is done, `pnpm -C app e2e` builds the app and the daemon, runs the
milestone 0 guarantees against a fresh test environment per test, and saves a
screenshot for each failure.

## Related code

- `app/src-tauri/Cargo.toml` and `app/src-tauri/src/main.rs` — the `webdriver`
  feature (uncommitted) registers `tauri-plugin-wdio-webdriver`. Release builds
  and `pnpm tauri dev` leave it out.
- `tauri-plugin-wdio-webdriver-1.2.0` — a W3C WebDriver server inside the app,
  on `127.0.0.1` at `$TAURI_WEBDRIVER_PORT` (default 4445). On macOS,
  screenshots come from the `WKWebView` snapshot API, so they need no screen
  recording permission, and setting the window rect resizes the window. Its
  key actions send synthetic `KeyboardEvent`s whose `keyCode` is the character
  code, which xterm.js reads as function keys, and each character arrives
  twice.
- `@xterm/xterm` 6.0.0 `_inputEvent` — an `InputEvent` with `inputType`
  `insertText`, dispatched on `.xterm-helper-textarea`, goes straight to
  `onData`, the handler real typing reaches.
- `crates/ur/tests/terminal.rs` — `Daemon` starts `ur daemon` in a `tempfile`
  directory with `UR_SOCKET`, `HOME`, and `SHELL=/bin/sh`, and kills it on drop.
  The test environment follows the same pattern.
- `app/src-tauri/src/gui_state.rs` — the GUI state file under
  `$XDG_STATE_HOME`, which each test environment points at its own directory.

## Decisions

### Runner and WebDriver client

Tests use Node's built-in `node:test`, run with `node --test`. Node 22 strips
TypeScript types itself, so the tests are TypeScript with no build step. The
WebDriver client is `webdriverio`'s `remote()`, connected to the embedded
server of an app the harness launches itself. `@wdio/tauri-service` is not
used: it starts the app once per session with an environment fixed in its
configuration, and these tests need a new test environment per test and must
close and reopen the GUI partway through one test.

`app/e2e/tsconfig.json` extends `app/tsconfig.json` with Node types, and
`pnpm -C app e2e` runs `tsc -p e2e` before the tests.

### What the suite runs against

`pnpm -C app e2e` runs `cargo build -p ur` and
`tauri build --debug --no-bundle --features webdriver`, which embeds the
webview in `ur-app` so no Vite server is needed. Both build with
`CARGO_TARGET_DIR` set to `target/e2e`. Without it, the app lands in
`target/debug/ur-app`, where `pnpm tauri dev` or another session's
`cargo build` replaces it with a build that loads a blank page when no Vite
server is running. `CARGO_TARGET_DIR` must be absolute: the Tauri CLI runs
Cargo from `app/src-tauri`, so a relative path resolves there.

### Test environment

Each test gets a `TestEnvironment`: a temporary directory holding the socket,
`HOME`, and the `XDG_STATE_HOME` directory, and a daemon started in it with
`UR_SOCKET`, `HOME`, `XDG_STATE_HOME`, and `SHELL=/bin/sh`. The GUI is started
with `UR_SOCKET`, `XDG_STATE_HOME`, and `TAURI_WEBDRIVER_PORT`. Tests run one
at a time (`--test-concurrency=1`) because they share the WebDriver port and
open windows.

Closing the GUI kills the `ur-app` process. The daemon sees what it sees when
the window closes: the socket connection ends.

Workspace directories and the config file are not part of this change.
Workspaces arrive in milestone 3 and the configured server in milestone 2; each
adds its directory to `TestEnvironment` then.

### Assertions

Tests assert on terminal text and on whether input works. Terminal text is read
from the xterm.js rows in the DOM (xterm.js's default renderer). There are no
screenshot comparisons: they break on style changes and invite tests of
appearance. `e2eTest` saves a screenshot to `app/e2e/artifacts/` when a test
fails, for diagnosis only.

### Typing

`Gui.type(text)` dispatches the `insertText` input event described under
Related code. WebDriver key actions are not used.

### Admission to the suite

`eng/testing.md` gets an end-to-end section holding the guarantee list, the
admission bar, and the non-goals. A guarantee is admitted only when it holds
across the daemon, the core, and the webview together and cannot be tested in
the Rust test suite, and only with the user's approval. Non-goals: styling,
layout, wording, component structure, and anything the Rust test suite already
covers. Ad-hoc checks import `app/e2e/harness.ts` from scripts outside the
repository, and their scripts and screenshots are not committed.

### The fake server

The fake server is not part of this change. Milestone 2 builds it when the
daemon first launches a server; `eng/todo.md` records that.

## Naming

- end-to-end suite — The WebDriver tests in `app/e2e/`, run by
  `pnpm -C app e2e`. Each test owns one guarantee from the end-to-end guarantee
  list.
- end-to-end guarantee list — The closed list of guarantees in `eng/testing.md`
  that the end-to-end suite owns.
- ad-hoc check — A throwaway script outside the repository that uses the harness
  to check behavior after a milestone. It is never committed.
- `TestEnvironment` — One test's temporary directory and daemon, and the GUI
  launches made in it.
- `Gui` — One running `ur-app` process and its WebDriver session: `type`,
  `lines`, `waitForLine`, `setWindowSize`, `screenshot`, and `close`.
- `e2eTest(name, body)` — Wraps `node:test`'s `test`: gives the body a started
  `TestEnvironment`, saves a screenshot on failure, and stops the environment.
- fake server — A server built with the SDK's `Agent.builder()` that the daemon
  launches from the config file in end-to-end tests. Named here so milestone 2
  uses the same term; added to `eng/glossary.md` when it exists.
- terminal, screen snapshot, daemon, core, webview, GUI state file — as defined
  in `eng/glossary.md`.

## Test plan

`app/e2e/terminal.test.ts` owns the two milestone 0 guarantees:

- `a terminal survives closing and reopening the GUI`: open the GUI and set the
  window to 700×450. Run `vi notes.txt`, insert two lines, and press Escape.
  Close the GUI and open it again at the default 1000×700, so it attaches at a
  different size. Both lines are on screen; `:set lines?` reports the view's
  row count; a third line typed now appears; `notes.txt` does not exist.
- `quitting a restored full-screen application returns to the shell`: open the
  GUI, run `top`, close the GUI, and open it again. Type `q`, then
  `echo back at the prompt`. The line `back at the prompt` appears and no row
  shows `top`'s `Processes:` header.

Each test fails with the terminal text in its message and a screenshot in
`app/e2e/artifacts/`.

## Implementation plan

### 1. Harness

- `app/package.json`: add `webdriverio` and `@types/node` as dev dependencies
  and an `e2e` script that builds the daemon and the app as above, runs
  `tsc -p e2e`, then `node --test --test-concurrency=1 e2e/`.
- `app/e2e/tsconfig.json`: as under Runner and WebDriver client.
- `app/e2e/harness.ts`: `TestEnvironment.start()` and `stop()`,
  `TestEnvironment.openGui()` (start `ur-app`, poll `/status` until the server
  answers, connect with `remote()`, wait for the terminal to render), `Gui`,
  and `e2eTest`. Binary paths resolve from `import.meta.dirname` to
  `target/e2e/debug/`.
- `.gitignore`: add `app/e2e/artifacts/`.

### 2. Milestone 0 guarantees

- `app/e2e/terminal.test.ts`: the two tests under Test plan.
- Run `pnpm -C app e2e` three times to confirm both tests pass every run.

## Documentation updates

- `eng/testing.md`: an End-to-end suite section with the end-to-end guarantee
  list (the two guarantees above), the admission bar, the non-goals, and ad-hoc
  checks, as under Admission to the suite.
- `AGENTS.md`: map `app/e2e/harness.ts`, `app/e2e/terminal.test.ts`, and the
  `webdriver` feature. Under Validation, run `pnpm -C app e2e` at the end of
  each milestone, and do not add end-to-end tests without the user's approval.
- `eng/glossary.md`: add end-to-end suite, end-to-end guarantee list, and ad-hoc
  check.
- `eng/todo.md`: milestone 2, task 4 also builds the fake server, shares its
  scripted behavior with the test agent, and adds the config file to
  `TestEnvironment`.
