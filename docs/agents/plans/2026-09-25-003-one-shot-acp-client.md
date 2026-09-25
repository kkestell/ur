# One-shot ACP client

## Goal

Build the One-shot ACP client section of `docs/agents/todo.md`.
`ur agent-run <workspace> <prompt>` reads the server command from the config
file, starts the server, sends `initialize`, `session/new`, and
`session/prompt`, prints each ACP update as one JSON line, and asks on stdin
when the server requests permission. When the prompt returns, it prints the
stop reason and exits.

When this is done, the check in `docs/agents/todo.md` passes with Ox: in Ask mode (Ox's
default mode), a prompt that runs `ls` stops at a permission request, approving
it prints the tool call and the answer, and the last line is `end_turn`.

## Related code

- `crates/ur/src/main.rs` — the clap command line; gains `agent-run`.
- `agent-client-protocol-2.1.0/examples/yolo_one_shot_client.rs` — the SDK's
  one-shot client: `Client.builder()` with a notification handler and a
  permission handler, then `connect_with` to an `AcpAgent` and the three
  requests through `block_task()`. `one_shot.rs` follows it.
- `agent-client-protocol-2.1.0/src/acp_agent.rs` — `AcpAgent::new(
  AcpAgentConfig::new(command).args(args))` launches the server from the
  config file. Dropping the ACP connection kills the server's process group.
- `agent-client-protocol-2.1.0/src/concepts/ordering.rs` — handlers hold the
  SDK's dispatch loop until they return; see Decisions.
- `agent-client-protocol-2.1.0/src/jsonrpc.rs` — `Channel::duplex()`, the
  in-process transport the tests use between `run` and a test agent.
- `~/projects/ox/src/sessions.rs` — Ox starts every session in its `ask` mode,
  so the check needs no config option.

## Decisions

### Output

Everything goes to stdout, one line at a time, in this order:

- The `InitializeResponse` as JSON. This is where the negotiated capabilities
  are kept: `agent-run` calls no optional method, so nothing else reads them.
- Each `SessionNotification`'s `update` as JSON, unchanged.
- For each permission request, the `RequestPermissionRequest` as JSON, then one
  line per permission option, `N. <name> (<kind>)`, numbered from 1.
- The stop reason, as its ACP name (`end_turn`, `cancelled`, ...).

A failed request (a JSON-RPC error from the server, or a server that cannot be
started) ends the command with that error and a non-zero exit status. The
server's stderr is not shown.

### Answering permission requests

The permission handler reads one line from the answers input. A number from 1
to the option count selects that option. Any other line prints
`answer with a number from 1 to N` and reads again. End of input sends
`session/cancel` for the session and answers `Cancelled`, as ACP requires for a
cancelled turn; the prompt then returns `cancelled`.

The handler holds the SDK's dispatch loop while it waits for the answer. The
server cannot continue the tool call until it gets the answer, and holding the
loop keeps the question the last thing printed. The handler waits only on the
answers input, never on the ACP connection, so it cannot deadlock. This is
specific to `agent-run`; the daemon's handlers only forward, as described
under Daemon architecture in `docs/agents/architecture.md`.

### Protocol version and client capabilities

`initialize` sends `ProtocolVersion::V1`, default `ClientCapabilities` (no
`fs/*` or `terminal/*`), and `client_info` with the name `ur` and the crate
version. If the response's `protocol_version` is not V1, `agent-run` fails
with an error naming the version, as the ACP spec asks of a client that does
not support the server's version.

### Config file

`config.rs` reads the config file, `$XDG_CONFIG_HOME/ur/config.toml`, else
`$HOME/.config/ur/config.toml`:

```toml
[server]
command = "ox"
args = []
```

`args` defaults to empty. Unknown keys are an error, so a typo is reported
instead of ignored. A missing or unreadable file, or a parse error, is an error
naming the path.

### Workspace path

`agent-run` makes the workspace argument absolute with `std::path::absolute`,
which does not resolve symlinks, and passes it as `cwd` to `session/new`. It
does not check that the path exists; the server's `session/new` error is
reported like any other.

### Testable entry point

`one_shot::run` takes the server as `impl ConnectTo<Client>`, the workspace
path, the prompt, the answers input as `impl AsyncBufRead`, and the output as
an `Arc<Mutex<W>>` where `W: Write`, and returns the `StopReason`.
`one_shot::start` reads the config file, builds the `AcpAgent`, calls `run`
with stdin and stdout, and prints the stop reason. Tests call `run` with a
test agent over `Channel::duplex()`.

## Naming

- server, config file, capabilities, one-shot client, session, turn, stop
  reason, permission option, cancellation, test agent — as defined in
  `docs/agents/glossary.md`.
- `Config { server: ServerConfig }` and `ServerConfig { command, args }` — the
  config file as read by `config.rs`.
- answers input — the line input `run` reads permission answers from; stdin in
  `start`, a byte string in tests.

## Test plan

Unit tests in `crates/ur/src/one_shot.rs` connect `run` to a test agent built
with `Agent.builder()` over `Channel::duplex()`. The test agent uses labels and
IDs unlike Ox's: a tool call titled `list things`, and permission options
`Go ahead` (`allow_once`, ID `go`) and `Hold off` (`reject_once`, ID `hold`).
On `session/prompt` it sends a `tool_call` update, asks permission from a
spawned task so its own dispatch loop stays free, then sends an
`agent_message_chunk` naming the chosen option ID and returns `end_turn`, or
returns `cancelled` when the outcome is `Cancelled`. It records the `cwd` and
prompt text it received.

- `runs_one_prompt_and_prints_each_update`: answers `1`. The output is the
  initialize response line, the `tool_call` update, the permission request and
  its two option lines, then the `agent_message_chunk` naming `go`; `run`
  returns `EndTurn`; the test agent received the absolute workspace path and
  the prompt text.
- `answers_permission_from_input`, table-driven over the answers input and the
  expected outcome: `1` selects `go`; `2` selects `hold`; `9` then `2` prints
  the retry line once and selects `hold`; empty input answers `Cancelled` and
  `run` returns `Cancelled`.
- `rejects_an_unsupported_protocol_version`: the test agent answers
  `initialize` with protocol version 2; `run` fails with an error naming it.

Run the check in `docs/agents/todo.md` with Ox by hand.

## Implementation plan

1. Root `Cargo.toml`: add `agent-client-protocol = "2.1.0"` and `toml` to the
   shared dependencies. `crates/ur/Cargo.toml`: add them and `serde`.
2. `crates/ur/src/config.rs`: `Config`, `ServerConfig`, and `Config::read()`
   as described under Config file.
3. `crates/ur/src/one_shot.rs`: `run` and `start` as described under Output,
   Answering permission requests, Protocol version and client capabilities,
   and Testable entry point. The notification handler and permission handler
   share the output through its mutex.
4. `crates/ur/src/main.rs`: add `AgentRun { workspace: PathBuf, prompt: String
   }`, documented as running one prompt against the configured server without
   a daemon, and call `one_shot::start`.
5. Add the tests in the Test plan.
6. Write a config file with `command = "ox"` and run the check in
   `docs/agents/todo.md`.

## Documentation updates

- `AGENTS.md`: map `crates/ur/src/config.rs` and `crates/ur/src/one_shot.rs`,
  and change the `main.rs` entry to list `daemon` and `agent-run`.
- `docs/agents/architecture.md`: under ACP boundary, show the config file's `[server]`
  table in place of the loose `command = "ox"` and `args = []`.
