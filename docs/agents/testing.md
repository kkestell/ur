# Testing

Run `cargo test --workspace` for the Rust test suite and `cargo build
--workspace` for a debug
build. `pnpm -C app test` runs the webview's unit tests under vitest.

Test ACP behavior against a test agent built with the SDK's `Agent.builder()` in the test process,
without a model provider or Ox. The daemon's tests run the fake server in process over
`Channel::duplex()`. Use Ox for live end-to-end checks, including each milestone's check in
`docs/agents/todo.md`. `make run` starts the daemon and the GUI for them; the CLI is
`target/debug/ur`.

## Test discipline

The test suite is curated code. Each test owns one durable, observable guarantee, and the suite
changes as the guarantees change.

- Give each guarantee one owning test, named for that guarantee. A test that checks unrelated
  guarantees is split so each failure names what broke.
- A change adds a test for each new guarantee and for each reproduced regression, at the closest
  stable boundary. It rewrites or deletes the tests for guarantees it changes or removes, in the
  same change.
- Test a guarantee once. Tests at different layers repeat an assertion only when those layers have
  distinct failure modes.
- Use table-driven cases for one behavior over varied inputs. Each case states its input and
  expected result, and a failure message identifies the case.
- Keep setup proportional to the guarantee. Shared fixtures and helpers hold setup that several
  tests need, so each test body reads as its guarantee.
- Test observable behavior. Internals change freely while the guarantees they serve hold.
- Before finishing any change that touches tests, inspect the complete test diff and report which
  guarantees gained, lost, or moved their owning tests.

## End-to-end suite

The end-to-end suite tests the GUI's behavior: the daemon, the core, and the webview together,
against the fake server. `pnpm -C app e2e` builds the daemon, the fake server, and the app, with its
embedded WebDriver server, into `target/e2e`, then runs the tests in `app/e2e/` one at a time. Each
test gets a `TestEnvironment`: a temporary directory for the socket, `HOME`, the GUI state file, the
fake server's saved history file, and a config file that launches the fake server, and a daemon
started in it. The saved history file lets sessions outlive a daemon restart. A failing test saves a
screenshot to `app/e2e/artifacts/` for diagnosis.

Tests drive the GUI the way the user does, through clicks, the editor, and the terminal, and assert
on what the window shows. `TestEnvironment.ur()` runs the command line for setup the GUI does
through native dialogs or menus, which WebDriver cannot drive, and `Gui.request()` sends a request
through the core's `request` command for the same reason. `Gui.showTerminal()` shows the terminal,
so its view is the only xterm.js on the page. Terminal tests assert on terminal text, read from the
xterm.js rows, and type through `Gui.type`, not WebDriver key actions. The fake server's prompt
scripts, named in `fake_server()`'s documentation, give agent session tests replies, permission
requests, and errors.

### End-to-end guarantee list

- A terminal survives closing and reopening the GUI: an editor's unsaved buffer is still on screen
  after the GUI reopens at a different window size, the editor sees the new size, and it accepts
  input.
- Quitting a restored full-screen application returns to the shell, and none of its last screen is
  left behind.
- The empty states follow the workspaces and the selection.
- A session created from the command line answers a prompt sent from the editor.
- Stop cancels a running turn.
- The selected session and its transcript survive closing and reopening the GUI.
- The GUI reconnects after a daemon restart without duplicating the thread, and a new prompt works.

### Admission

Every change to the GUI's behavior adds an end-to-end test for each behavior the user can see: what
the window shows and what the user's actions do. When the fake server lacks a behavior a test needs,
the change extends the fake server. Styling, wording, and component structure are not end-to-end
guarantees. There are no screenshot comparisons.

### Ad-hoc checks

Other checks, such as confirming a change in the running app, are ad-hoc checks: scripts outside the
repository that import `app/e2e/harness.ts`. Their scripts and screenshots are not committed.
