# Testing

Run `cargo test --workspace` for the Rust test suite and
`cargo build
--workspace` for a debug build.

Test ACP behavior against a test agent built with the SDK's `Agent.builder()` in
the test process, without a model provider or Ox. Use Ox for live end-to-end
checks, including each milestone's check in `eng/todo.md`.

## Test discipline

The test suite is curated code. Each test owns one durable, observable
guarantee, and the suite changes as the guarantees change.

- Give each guarantee one owning test, named for that guarantee. A test that
  checks unrelated guarantees is split so each failure names what broke.
- A change adds a test for each new guarantee and for each reproduced
  regression, at the closest stable boundary. It rewrites or deletes the tests
  for guarantees it changes or removes, in the same change.
- Test a guarantee once. Tests at different layers repeat an assertion only when
  those layers have distinct failure modes.
- Use table-driven cases for one behavior over varied inputs. Each case states
  its input and expected result, and a failure message identifies the case.
- Keep setup proportional to the guarantee. Shared fixtures and helpers hold
  setup that several tests need, so each test body reads as its guarantee.
- Test observable behavior. Internals change freely while the guarantees they
  serve hold.
- Before finishing any change that touches tests, inspect the complete test diff
  and report which guarantees gained, lost, or moved their owning tests.

## End-to-end suite

`pnpm -C app e2e` builds the daemon and the app, with its embedded WebDriver
server, into `target/e2e`, then runs the tests in `app/e2e/` one at a time. Each
test gets a `TestEnvironment`: a temporary directory for the socket, `HOME`, and
the GUI state file, and a daemon started in it. A failing test saves a
screenshot to `app/e2e/artifacts/` for diagnosis.

Tests assert on terminal text, read from the xterm.js rows, and on whether input
works. They type through `Gui.type`, not WebDriver key actions.

### End-to-end guarantee list

- A terminal survives closing and reopening the GUI: an editor's unsaved buffer
  is still on screen after the GUI reopens at a different window size, the
  editor sees the new size, and it accepts input.
- Quitting a restored full-screen application returns to the shell, and none of
  its last screen is left behind.

### Admission

A guarantee joins the list only when it holds across the daemon, the core, and
the webview together and cannot be tested in the Rust test suite, and only with
the user's approval. Styling, layout, wording, component structure, and anything
the Rust test suite already covers are not end-to-end guarantees. There are no
screenshot comparisons.

### Ad-hoc checks

Other checks, such as confirming a change in the running app, are ad-hoc checks:
scripts outside the repository that import `app/e2e/harness.ts`. Their scripts
and screenshots are not committed.
