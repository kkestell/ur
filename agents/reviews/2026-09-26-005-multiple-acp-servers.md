# Multiple ACP servers

## Scope and coverage

Reviewed commit `5870e39`, "Support multiple named ACP servers": the config file and one-shot
selection, `ServerControl`, the per-server supervisors and generations in `State`, session keys in
the protocol and ops, the webview's server settings, creation menus, shortcut, labels, and
per-session capabilities, the end-to-end harness and its menu and dialog wrappers, the fake server's
capability flags, and the documentation the commit changed. The plan
`plans/2026-09-26-004-multiple-acp-servers.md` defined the required behavior.

Lenses: correctness, concurrency, testing, simplicity, readability, comments, and documentation.

Two sessions reviewed this commit at the same time, and their findings are combined here. The review
did not check whether requests still in flight when a server is removed or restarted, such as
`session/new` and `session/set_config_option`, get answered. That depends on how the ACP SDK treats
a connection whose task was aborted.

## Fixed

- **Correcting a server leaves settings open with a stale error**
  (`crates/ur/src/daemon/state.rs:131`): Saving a new executable or new arguments restarts the
  server. The restart recorded "the server configuration changed" as that server's error. The
  settings screen's wait for the connection read this as a failure: it stopped waiting and showed
  the message in red, even after the server connected. An ad-hoc end-to-end check reproduced this.
  It saved `/no/such/server`, then saved the fake server, and settings stayed open with that error
  while watch reported the server connected. `configure_server()` now clears the error when the
  launch configuration changes. The end-to-end tests missed this because `Gui.click` clicks through
  the settings overlay. `server-setup.test.ts` and `multiple-servers.test.ts` now wait for settings
  to close after the correction.
- **Stale single-connection doc comments** (`crates/ur/src/daemon/state.rs:19`, `:124`): The `State`
  doc named "the daemon's ACP connection". The doc on `set_config_error()` described setting an ACP
  connection and starting a generation. Both now describe what the code does.
- **Stale project layout** (`agents/architecture.md:256`): `config.rs` and `mod.rs` were described
  as serving one server with one supervisor. They now describe named servers, one supervisor per
  server, and `ServerControl`.
- **Repeated server lookups** (`app/src/store/watch.ts:124`): Each sidebar row repeated
  `watch.servers.find(...)` three times, and `SessionPanel` nested one lookup inside another.
  `sessionServer()` now does the lookup in `Sidebar.tsx`, `Tab.tsx`, and `Layout.tsx`.
- **Hand-copied configs** (`crates/ur/src/config.rs:10`, `crates/ur/src/daemon/mod.rs:51`): Four
  hand-built `Config { servers: x.servers.clone() }` copies and an `async move` wrapper around
  `notified()` were needless. `Config` now derives `Clone`.
- **Pointless rebinding** (`crates/ur/src/daemon/ops.rs:178`): `load()` rebound its `server`
  parameter as `owner`. The parameter is now named `owner`.
- **README says menus list only connected servers** (`README.md:19`): The creation menus list every
  configured server and disable the ones that are not connected. The README now says so.
- **Glossary says the daemon has one ACP connection** (`agents/glossary.md:66`): The daemon entry
  now says it owns the ACP connections.

## Findings

### Medium

#### Testing

- **OX-0014 Server removal and launch edits have no daemon tests**
  (`crates/ur/src/daemon/mod.rs:85`): `ServerControl::add()`, `update()`, and `remove()`, and
  `State::remove_server()`, are untested in Rust. These paths do new work: they settle waiting
  deletes, cancel permission requests, end loads, and ignore late results from a retired connection.
  A regression there would drop a response or leave a session stuck, and no test would fail.
  `tests.rs` has no `AddServer`, `UpdateServer`, `RemoveServer`, or `remove_server`, and
  `two_servers()` serves with no `ServerControl`. The plan's test plan requires several daemon tests
  that are missing: removal and launch edits while operations are pending, late results from a
  retired connection, a failed config write leaving the running configuration intact, and the
  startup timeout for a stalled first attempt. The work log does not list any of these as a
  departure from the plan. To fix, give `TestDaemon` a `ServerControl` whose launch can be replaced.
  Then add one test each for removal during a turn, a permission request, and a waiting delete; a
  launch edit during a load; and a failed write.

### Low

#### Simplicity

- **OX-0015 The fake server saves its capability flags in its history file**
  (`crates/ur-fake-server/src/lib.rs:50`): `image` and `delete` are written to the saved history
  file. `main.rs` overwrites them from its flags on every start, and `capabilities()` rewrites the
  file each launch, so the saved values are never used. To fix, keep the flags out of the saved
  history, with `#[serde(skip)]` and defaults or a separate field on `SavedHistory`.

## Checks run

- Ad-hoc end-to-end check: correcting a failed server. Before the fix, settings stayed open showing
  "the server configuration changed". After the fix, settings closes.
- `cargo test -p ur` — passed.
- `make check` — passed.
- `make check-docs` — passed.
- `make e2e` — two runs, and neither passed fully. The first had 78 of 85 passing, with seven
  failures in `attention.test.ts`; the second had 84 of 85 passing, with the usage indicator test in
  `editor.test.ts` failing. Rerun alone, `attention.test.ts` passed 9 of 9 and `editor.test.ts`
  passed 12 of 12. Both full runs passed `server-setup.test.ts` and `multiple-servers.test.ts`.
  Another session was also running this repository's app during the first run.

## Verdict

The multiple-server support works for the ordinary workflow. The one user-visible bug, settings
staying open with a stale error after correcting a server, is fixed and covered by the end-to-end
tests. The daemon's server removal and launch-edit paths still need the tests the plan called for.
