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
started in it with one workspace, `home`, at the test's `HOME`. The saved history file lets sessions
outlive a daemon restart. A failing test saves a screenshot to `app/e2e/artifacts/` for diagnosis.

Tests drive the GUI the way the user does, through clicks, the editor, and the terminal, and assert
on what the window shows. `TestEnvironment.ur()` runs the command line for setup the GUI does
through native dialogs or menus, which WebDriver cannot drive, and `Gui.request()` sends a request
through the core's `request` command for the same reason. `openGui()` makes `ur-app` the frontmost
application with `Gui.focusWindow()`, since the GUI focuses its visible sessions only while its
window has focus and WebDriver cannot focus the window; a test that depends on focus calls it again
before that step. `Gui.showTerminal()` opens a terminal in `home` and selects its terminal row, so
its view is the only xterm.js on the page. Terminal tests assert on terminal text, read from the
xterm.js rows, and type through `Gui.type`, not WebDriver key actions. The fake server's prompt
scripts, named in `fake_server()`'s documentation, give agent session tests replies, permission
requests, and errors.

### End-to-end guarantee list

- A terminal survives closing and reopening the GUI: an editor's unsaved buffer is still on screen
  after the GUI reopens at a different window size, the editor sees the new size, and it accepts
  input.
- Quitting a restored full-screen application returns to the shell, and none of its last screen is
  left behind.
- A terminal row shows the shell's name until a program sets a title.
- A terminal starts in its workspace's directory.
- Close Terminal removes the terminal's row.
- A terminal whose shell exits leaves the sidebar.
- Removing a workspace stops its terminals.
- The empty states follow the workspaces and the selection.
- A session created from the command line answers a prompt sent from the editor.
- Stop cancels a running turn.
- The selected session and its transcript survive closing and reopening the GUI.
- The GUI reconnects after a daemon restart without duplicating the thread, and a new prompt works.
- A prompt the daemon answers busy comes back to the editor.
- An editor draft does not follow the selection to another session.
- A session that comes back with its workspace shows its thread.
- Another session's activity leaves the thread's scroll position alone.
- The session header shows the session title.
- The session header's + creates a session in its workspace and selects it.
- Deleting a session removes it from the sidebar.
- A session holding 20 MB of images loads after reopening the GUI.
- A working session shows the spinner.
- A session waiting for permission shows its mark and its workspace's count.
- Clicking a permission option answers the request.
- A permission request shows its tool call's content.
- The permission shortcuts answer the oldest request.
- A session that finishes a turn while not shown is unread until it is shown.
- A failed turn shows its error and the failed mark.
- A rejected prompt shows its error under the user message.
- Agent messages render as Markdown.
- The copy button copies the message's Markdown source.
- A Thinking row shows its thought when clicked and hides it when clicked again.
- A Run Command block shows its output when clicked.
- A tool call row shows its content when clicked.
- Clicking a link in an agent message leaves the app in place.
- Typing / lists the server's slash commands, and Enter inserts one.
- Choosing a config option value sets it on the server.
- A config option the server changes updates its picker.
- Usage the server reports shows in the usage indicator.
- An image dropped on the editor is sent with the prompt.
- A dropped file that is not an image is refused.

### Admission

Every change to the GUI's behavior adds an end-to-end test for each behavior the user can see: what
the window shows and what the user's actions do. When the fake server lacks a behavior a test needs,
the change extends the fake server. Styling, wording, and component structure are not end-to-end
guarantees. There are no screenshot comparisons.

### Ad-hoc checks

Other checks, such as confirming a change in the running app, are ad-hoc checks: scripts outside the
repository that import `app/e2e/harness.ts`. Their scripts and screenshots are not committed.
