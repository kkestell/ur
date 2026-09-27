# Testing

`make check` runs the Rust tests, the webview's unit tests, and the other checks. `make e2e` runs
the end-to-end suite.

## Where tests go

- **Daemon and protocol behavior**: Rust tests against the fake server, run in process. They need no
  model provider and no Ox.
- **Pure webview logic**, such as the transcript reducer: unit tests under vitest.
- **GUI behavior**: the end-to-end suite, which runs the daemon, the core, and the webview together
  against the fake server. A behavior the fake server lacks is added to the fake server.
- **Live checks against Ox**: by hand, with `make run`, for the checks listed in `todo.md`.

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

Each test runs in its own temporary environment with its own socket, home directory, config, and
state, so tests share nothing. Tests drive the GUI the way the user does and assert on what the
window shows. Native menus and dialogs, which WebDriver cannot reach, are replaced in the end-to-end
build by versions the tests can answer. Setup that the user would do through them goes straight to
the daemon socket.

On macOS the suite runs in the background without taking focus. A failing test saves a screenshot to
`app/e2e/artifacts/`.
