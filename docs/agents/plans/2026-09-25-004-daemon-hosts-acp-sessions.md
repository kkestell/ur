# Daemon hosts ACP sessions

## Goal

Build the Daemon hosts ACP sessions section of `docs/agents/todo.md`. The daemon launches the server
from the config file and keeps one ACP connection. Daemon clients create sessions, send prompts, and
subscribe to a session's transcript: a session snapshot followed by every later transcript entry, in
order, with no gaps or duplicates. A prompt sent while the session's operation guard is held answers
busy and changes nothing. The CLI gains `ur new <path>`, `ur prompt <session> <text>`, and
`ur read <session> [--follow]`.

When this is done, the check in `docs/agents/todo.md` passes with Ox: `ur read --follow` streams the
reply to a prompt sent with `ur prompt`, and a second `ur read --follow` started partway through
shows the whole transcript once.

## Related code

- `crates/ur/src/daemon/mod.rs` — `start()` binds the socket; gains the config file and the ACP
  connection.
- `crates/ur/src/daemon/server.rs` — the accept loop, `Outbox`, and `handle()`; gains the session
  requests.
- `crates/ur/src/daemon/terminal.rs` — `Terminals::attach` queues the screen snapshot and registers
  the outbox under one lock, and the reader thread queues output to outboxes under that lock.
  Subscribe follows the same pattern.
- `crates/ur/src/one_shot.rs` — `prompt_once` sends `initialize` and checks the protocol version;
  the daemon reuses that step. Its tests show a test agent over `Channel::duplex()`.
- `crates/ur/src/config.rs` — `Config::read()`, shared with the daemon.
- `crates/ur-client/src/protocol.rs` and `client.rs` — the wire protocol types and `Client`, which
  today drops every event except `terminal_exited`.
- `agent-client-protocol-2.1.0/src/concepts/ordering.rs` — handlers hold the SDK's dispatch loop
  until they return, and an `on_receiving_result` callback holds it for a response. See Decisions.
- `agent-client-protocol-2.1.0/src/jsonrpc.rs` — `SentRequest::
  on_receiving_result`,
  `ConnectionTo::incoming_closed`, and `Responder::respond_with_internal_error`.
- `agent-client-protocol-2.1.0/examples/simple_agent.rs` — an `Agent.builder()` served over
  `Stdio::new()`, the shape of the fake server's binary.
- `app/e2e/harness.ts` — `TestEnvironment` starts the daemon; it gains the config file.
- `~/projects/ox/src/acp.rs` — Ox sends `available_commands_update` right after its `session/new`
  response, which is the case the ordering decision covers.

## Decisions

### Handlers apply ACP updates to `State` directly

This replaces the ingest task in `docs/agents/architecture.md`. The notification handler locks
`State`, appends the ACP update entry, queues the `entry` event to the session's subscribers, and
returns. It never awaits, so it holds the SDK's dispatch loop only for the lock.

Responses use `on_receiving_result`, never `block_task()`. The SDK runs that callback before it
dispatches the next message from the server. So the `session/new` callback adds the session to
`State` before the server's next update for it is handled, and the `session/prompt` callback clears
the operation guard only after the turn's last update is in the transcript. The callbacks always
return `Ok(())`, because an error from one shuts down the ACP connection.

### Events are queued under the `State` lock

`State` methods queue their events on the affected outboxes with `try_send` while the lock is held,
as `Terminals` does for output. A subscriber's session snapshot and every later entry are queued
under one lock, in transcript order. `try_send` never waits, so `State` still does no IO and no
awaiting. A closed or full outbox is dropped from the subscriber list, as for terminals.

### The daemon keeps running without a server

`daemon::start` binds the socket, then reads the config file, launches the server, and waits for
`initialize` before accepting socket connections. Daemon clients that connect meanwhile wait in the
listen backlog, so no request sees a server that is still starting.

If the config file cannot be read, the server cannot start, `initialize` fails, or the server later
exits, the daemon logs the reason to stderr and keeps running. Terminals keep working, subscribe
still serves the transcripts held in memory, and `new_session` and `prompt` answer an error naming
the reason. The daemon does not restart the server, and a request in flight when the server exits
may go unanswered; the Restore agent history section of `docs/agents/todo.md` handles server exit.

### Prompts return when sent

`prompt` answers `done` once the prompt is sent, or `busy` if the session's operation guard is held,
and the turn runs on. `ur prompt` exits then; `ur read --follow` shows the reply. The stop reason is
not kept, since session status comes later. A JSON-RPC error from `session/prompt` is logged to the
daemon's stderr and clears the guard; turn error entries come later.

The prompt op, under the `State` lock: fail if there is no ACP connection or no such session, answer
busy if `session.op` is set, send `session/prompt` and register its callback, then set `session.op`
and append the user prompt entry. Sending under the lock means a failed send leaves nothing to undo,
and the server's first update waits for the lock, so it follows the user prompt entry.

### Permission requests are rejected

The permission handler answers every `session/request_permission` with
`respond_with_internal_error("ur does not answer permission requests yet")`. It stores nothing.

### Wire protocol

Only the variants this section needs. `session` fields are the ACP `SessionId`.

- `Request::NewSession { path: PathBuf }` → `Response::SessionCreated {
  session }`, sent from the
  `session/new` callback. `path` is passed as `cwd` unchanged; `ur new` makes it absolute with
  `std::path::absolute`.
- `Request::Subscribe { session }` → `Event::SessionSnapshot { session,
  transcript: Vec<Entry> }`,
  queued before `Response::Done`. Subscribing again from the same socket connection sends a fresh
  snapshot and replaces the earlier subscription.
- `Request::Prompt { session, content: Vec<ContentBlock> }` → `Response::Done` or `Response::Busy`.
- `Event::Entry { session, entry }` — one transcript entry, after the snapshot.
- `Entry`, tagged by `type`: `Update { update: SessionUpdate }` for an ACP update entry and
  `UserPrompt { content: Vec<ContentBlock> }` for a user prompt entry.
- An unknown session, or no ACP connection, answers `Response::Error`.

`ur-client` depends on `agent-client-protocol-schema = "=1.7.0"` with default features off, the
version the SDK pins, so these are the SDK's own schema types. ts-rs overrides the ACP fields:
`SessionId` as `string`, and `SessionUpdate` and `ContentBlock` as
`import("@agentclientprotocol/sdk").SessionUpdate` and `...ContentBlock`. The app gains
`@agentclientprotocol/sdk` 1.5.0 as a dev dependency so `pnpm -C app build` checks the bindings.

`Client::events()` returns a receiver for every event, replacing an earlier receiver, as `pty()`
does. `terminal_exited` still closes the terminal's `pty` receiver.

### CLI output

- `ur new <path>` prints the session ID.
- `ur prompt <session> <text>` sends one text block and prints nothing. Busy exits non-zero with
  `session <session> is busy`.
- `ur read <session>` prints each transcript entry of the session snapshot as one JSON line and
  exits. With `--follow`, it then prints each `entry` event as one JSON line until the daemon closes
  the socket connection.

Every daemon error is printed and exits non-zero. A failed connection names the socket path.

### Fake server

`crates/ur-fake-server` is a workspace crate. `lib.rs` exports
`fake_server(hold: Hold) -> impl ConnectTo<Client>`, the scripted server built with
`Agent.builder()`. `main.rs` serves it over `Stdio::new()`. The daemon's tests run it in process
over `Channel::duplex()`; the end-to-end harness launches the binary from the config file. `ur` uses
the library only as a dev-dependency.

It advertises protocol version 1 and no optional capabilities, so it is also the minimal server with
no history or image capabilities. It assigns session IDs `fake-1`, `fake-2`, and so on, and sends an
`available_commands_update` with one command, `tally`, right after each `session/new` response. Its
labels, options, and tool inputs are unlike Ox's. `session/prompt` runs the script named by the
prompt's text:

- `hold`: sends the agent message chunk `holding`, waits for `Hold::release()`, sends `released`,
  and returns `end_turn`. The binary never releases it.
- `tool`: sends a tool call `tally-1` titled `count the tallies`, kind `search`, with `raw_input`
  `{"glob": "*.tally"}`, then asks permission with the options `Go ahead` (`allow_once`, ID `go`)
  and `Hold off` (`reject_once`, ID `stop`). A selected option completes the tool call and sends a
  message naming the option ID; `Cancelled` returns `cancelled`; an error fails the tool call and
  sends a message with the error. Otherwise it returns `end_turn`.
- Any other text: sends `you said: <text>` as one agent message chunk per word and returns
  `end_turn`.

The prompt handler runs the script in a task spawned on its connection, so its dispatch loop stays
free while it waits for a permission answer or the hold.

## Naming

- server, ACP connection, daemon client, session, transcript, transcript entry, ACP update entry,
  user prompt entry, session snapshot, subscribe, operation guard (`session.op`), busy, outbox,
  `State`, test agent, config file — as defined in `docs/agents/glossary.md`.
- fake server — the scripted server in `crates/ur-fake-server`: its binary is what the daemon
  launches in end-to-end tests, and its library is the test agent in the daemon's tests. Added to
  `docs/agents/glossary.md`.
- script — what the fake server does for a prompt, chosen by the prompt's text: `hold`, `tool`, or
  the reply.
- `Hold` — the fake server's handle that ends a `hold` script with `release()`.
- `Entry`, `Event::Entry`, `Event::SessionSnapshot`, `Response::SessionCreated`, `Response::Busy`,
  `Request::NewSession`, `Request::Subscribe`, `Request::Prompt` — the wire protocol names above.

## Test plan

Tests in `crates/ur/src/daemon/tests.rs` run the daemon in process: the test binds a socket in a
temporary directory, runs the daemon on it with the fake server over `Channel::duplex()`, and
connects `ur_client::Client`s. Shared helpers create a session, subscribe and return the snapshot
and events, and wait for the next `entry` event with a timeout.

- `subscribe_sends_the_transcript_then_live_entries`: the first daemon client subscribes and prompts
  `hold`; after it receives `holding`, a second daemon client subscribes, and its snapshot is the
  user prompt entry and `holding`. After `release()`, both receive `released`. The first daemon
  client's entries and the second's snapshot followed by its entries both equal the transcript: user
  prompt entry, `holding`, `released`.
- `overlapping_prompt_is_busy`: while `hold` runs, a second prompt answers `busy`; a new
  subscriber's snapshot has one user prompt entry; after `release()` the first turn still sends
  `released`.
- `updates_right_after_session_new_are_kept`: a subscriber's snapshot of a new session holds the
  fake server's `available_commands_update`.
- `permission_requests_are_rejected`: prompting `tool` yields the tool call, a failed
  `tool_call_update`, and a message containing the daemon's rejection.
- `session_requests_fail_without_a_server`: a daemon run with a config error answers `new_session`
  with an error containing it, and `open_terminal` still works.

The terminal integration tests in `crates/ur/tests/terminal.rs` stay as they are; their daemon has
no config file. The end-to-end suite gains no tests; its daemon now launches the fake server, and
the terminal tests must still pass.

Run the check in `docs/agents/todo.md` with Ox by hand.

## Implementation plan

The groups follow the numbered tasks in the section of `docs/agents/todo.md`.

Sessions and prompts:

1. Root `Cargo.toml`: add `crates/ur-fake-server` to the members, and `agent-client-protocol-schema`
   and `ur-fake-server` to the shared dependencies. `crates/ur-client/Cargo.toml`: add
   `agent-client-protocol-schema`.
2. `crates/ur-client/src/protocol.rs`: `Entry`, `Request::NewSession`, `Request::Prompt`,
   `Response::SessionCreated`, `Response::Busy`, and `Event::Entry`, with the ts-rs overrides.
   `app/package.json`: add `@agentclientprotocol/sdk`. Regenerate `app/src/ipc/bindings/` with
   `cargo test`.
3. `crates/ur/src/daemon/state.rs`:
   `State { server: Result<ConnectionTo<Agent>,
   String>, sessions: HashMap<SessionId, Session> }`
   and `Session { transcript,
   op: bool, subscribers: Vec<Outbox> }`, with methods to set the
   server, add a session, apply an update (an unknown session is logged and dropped), start a
   prompt, and finish a prompt.
4. `crates/ur/src/daemon/acp.rs`: `connect(server, state)` spawns `Client.builder()` with the
   notification and permission handlers, `connect_with` sends `initialize`, stores the connection in
   `State`, and waits for `incoming_closed()`; when the ACP connection ends, it stores the reason.
   Move the `initialize` request and protocol version check from `one_shot::prompt_once` into
   `acp::initialize`, and call it from both.
5. `crates/ur/src/daemon/ops.rs`: `new_session` and `prompt` as described under Decisions, each
   registering an `on_receiving_result` callback.
6. `crates/ur/src/daemon/mod.rs`: `start` binds the socket and calls
   `run(listener, server: anyhow::Result<impl ConnectTo<Client>>)`, which calls `acp::connect` or
   logs the error, then serves. `server.rs`: pass the `State` to `handle`, which returns
   `Option<Response>`; `None` means the op answers later.

Subscribe and the operation guard:

7. `protocol.rs`: `Request::Subscribe` and `Event::SessionSnapshot`. `state.rs`: `subscribe` queues
   the snapshot and registers the outbox under the lock; `start_prompt` answers busy when `op` is
   set.
8. `crates/ur-client/src/client.rs`: `Client::events()`.

CLI:

9. `crates/ur/src/cli/mod.rs` with `connect()`, and `new.rs`, `prompt.rs`, and `read.rs`, as
   described under CLI output. `crates/ur/src/main.rs`: add `New { path }`,
   `Prompt { session, text }`, and `Read { session, follow }`.

Tests and the fake server:

10. `crates/ur-fake-server/Cargo.toml`, `src/lib.rs`, and `src/main.rs`, as described under Fake
    server. `crates/ur/Cargo.toml`: add it as a dev-dependency.
11. `crates/ur/src/daemon/tests.rs`: the tests in the Test plan.
12. `app/e2e/harness.ts`: `TestEnvironment` writes `config/ur/config.toml` in its directory with
    `command` set to `target/e2e/debug/ur-fake-server`, and starts the daemon with `XDG_CONFIG_HOME`
    pointing at it. `app/package.json`: the `e2e` script builds `-p ur -p ur-fake-server`.
13. Run the check in `docs/agents/todo.md` with Ox and the end-to-end suite.

## Documentation updates

- `AGENTS.md`: map `daemon/state.rs`, `daemon/acp.rs`, `daemon/ops.rs`, `daemon/tests.rs`, `cli/`,
  and `crates/ur-fake-server`; add `new`, `prompt`, and `read` to the `main.rs` entry; update the
  `protocol.rs`, `client.rs`, and `harness.ts` entries.
- `docs/agents/architecture.md`: under Daemon architecture, replace the ingest task with handlers
  that apply updates directly, `State` methods that queue events under the lock, and ops that handle
  responses in `on_receiving_result` callbacks, with the ordering reasons above. Update the Runtime
  section's `block_task()` sentence to match, remove `ingest.rs` from the project layout, change
  `mod.rs`'s description, and add `crates/ur-fake-server`.
- `docs/agents/glossary.md`: remove Ingest task, drop `AcpIncoming` from Generation, change `State`
  to say its methods queue events on outboxes, and add fake server.
- `docs/agents/testing.md`: `TestEnvironment` writes a config file that launches the fake server;
  daemon tests use the fake server in process.
