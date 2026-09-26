# Multiple ACP servers, second pass

## Scope and coverage

Reviewed commit `5870e39`, "Support multiple named ACP servers", a second time, after the first
review in `reviews/2026-09-26-005-multiple-acp-servers.md`. This pass reviewed the code as it stands
at `782394f`, which includes the first review's fixes. It traced `ServerControl`, the per-server
supervisors and generations, `State`'s server removal and exit handling, session keys in the
protocol and ops, the ACP SDK's process cleanup and response callbacks, the webview's server
settings, creation menus, shortcut, and per-session capabilities, the end-to-end menu and dialog
wrappers, and the daemon and end-to-end tests. The plan
`plans/2026-09-26-004-multiple-acp-servers.md` defined the required behavior.

Lenses: correctness, concurrency, resources, testing, simplicity, and comments.

Removing a server or editing its launch configuration aborts the supervisor task. The ACP SDK kills
the server's process group when its connection is dropped, so no process is left behind. The SDK
runs each response callback as a task of the connection, so aborting the connection probably drops
the callbacks of requests still in flight, such as `session/new` and `session/set_config_option`,
and leaves those daemon requests unanswered. This review did not confirm that, because the daemon
tests have no `ServerControl` to drive a removal (OX-0014).

## Fixed

- **Exit and reload comments describe every session** (`crates/ur/src/daemon/state.rs:307`, `:709`):
  The `server_exited()` doc said every session becomes unloaded and that pending permission requests
  are dropped. Only the exited server's sessions change, and their requests are answered
  `Cancelled`. The `reload_subscribed()` doc also described every session. Both now describe the one
  server's sessions.
- **Workspace listing comment names one server** (`crates/ur/src/daemon/ops.rs:30`): The
  `add_workspace()` doc said a task lists the workspace's sessions when "the server" can list them.
  It now says one task runs for each connected server that can list sessions.
- **Server requests and events have no doc comments** (`crates/ur-client/src/protocol.rs:86`, `:89`,
  `:185`, `:218`): Every other request, response, and event documents its behavior, but `AddServer`,
  `UpdateServer`, `RemoveServer`, `ServerAdded`, and `ServersChanged` had none, and the `Watch` doc
  left servers out of the changes it sends. Each now states what it does, including the
  absolute-path requirement, which edits restart a server, and that removal leaves the sessions on
  the server.
- **Three copies of the fake server launcher** (`crates/ur/src/daemon/tests.rs:128`):
  `TestDaemon::run_fake()` and `two_servers()` each built the same closure that starts a fake server
  and records its task. `fake_launch()` now builds it for all three.

## Findings

No new open findings. OX-0014, from the first review, still stands: server removal and launch edits
have no daemon tests.

## Checks run

- `cargo test -p ur -p ur-client` — passed.
- `make check` — passed.

The fixes change only comments and test code, so `make e2e` was not run.

## Verdict

The multiple-server support holds up on a second pass. This pass fixed stale comments, documented
the server requests, and removed duplicated test setup. The remaining work is OX-0014: daemon tests
for server removal and launch edits, which would also settle whether requests in flight during a
removal get answered.
