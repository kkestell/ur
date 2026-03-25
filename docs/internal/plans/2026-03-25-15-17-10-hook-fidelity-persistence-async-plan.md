# Hook Fidelity, Persistence Ordering, and Async Runtime

## Goal

Fix three blocking gaps between the documented extension contract and the
implementation: (1) hook call-sites that pass summary fields instead of the
actual mutable data, (2) `TurnComplete` being emitted after persistence so
it is never written until the next turn, and (3) async HTTP functions that
are unreachable from synchronous tool/hook dispatch.

## Desired outcome

- Every hook call-site passes exactly the fields listed in the brainstorm
  contract, and every returned mutation is applied before execution proceeds
- `TurnComplete` is persisted in the same `persist_and_compact` call as the
  rest of the turn
- Lua tool handlers and hook handlers can call `ur.http.get`/`ur.http.post`
  and have the futures actually resolve
- A new integration test proves the network capability works end-to-end
  from a Lua tool handler
- `make verify` passes

## Related documents

- `docs/internal/brainstorms/2026-03-25-08-46-11-lua-extension-pivot-brainstorm.md`
- `docs/internal/brainstorms/2026-03-23-16-15-50-react-loop-hooks-brainstorm.md`
- `docs/internal/plans/2026-03-25-08-48-31-lua-extension-pivot-plan.md`

## Related code

- `src/session.rs` — turn loop, hook call-sites, persist_and_compact, event ordering
- `src/hooks.rs` — hook dispatch, HookResult
- `src/host_api.rs` — ur module builder, ur.http async functions
- `src/lua_host.rs` — LuaExtension::call_tool, call_hook (sync dispatch)
- `src/types.rs` — Message, ConfigSetting, SessionEvent (serde structs passed to hooks)
- `src/main.rs` — entry point, no tokio runtime
- `src/providers/google.rs` — `Handle::current().block_on()` pattern
- `extensions/workspace/test-extension/init.lua` — test extension exercising hooks

## Current state

### Issue 1: Hook contexts are incomplete / mutations discarded

The brainstorm contract specifies exact input fields and mutable fields for
each hook point. Current implementation deviates:

| Hook | Contract says | Code actually passes | Mutation applied? |
|------|-------------|---------------------|-------------------|
| `before_completion` | messages, model, settings, tools | model, provider, tool_count | model only |
| `after_completion` | messages, model, response → mutate response | model, provider, response | `_after_ctx` discarded |
| `after_session_load` | session_id, messages → mutate messages | session_id, event_count | `_after_ctx` discarded |
| `before_session_append` | session_id, message → mutate message | session_id, event_type (string) | reject-only; no message body |
| `before_compaction` | messages → mutate messages | message_count (integer) | reject-only; no messages |
| `after_compaction` | original, compacted → mutate compacted | original_count, compacted_count | `_after_ctx` discarded |

Hooks that should allow content mutation (`after_completion`,
`after_session_load`, `before_session_append`, `before_compaction`,
`after_compaction`) currently serve only as observability points because
they receive summary scalars instead of the serialized data.

### Issue 2: TurnComplete not persisted

`run_turn()` calls `persist_and_compact()` at line 349, then appends
`TurnComplete` at line 351. `persist_and_compact` writes
`self.events[self.persisted_event_count..]` and advances the counter. So
`TurnComplete` is only written to storage if a subsequent turn triggers
another `persist_and_compact`.

### Issue 3: Async HTTP unreachable from sync dispatch

`ur.http.get` and `ur.http.post` are registered with
`lua.create_async_function`, making them Lua coroutines under the hood.
`LuaExtension::call_tool` and `call_hook` call `handler_key.call(args)` —
a synchronous `LuaFunction::call`. When a sync call encounters a yielding
coroutine, mlua returns `LuaError::CoroutineInactive` or similar. No tokio
runtime wraps `main()` either, so even if we switched to async dispatch the
futures wouldn't resolve.

## Constraints

- **Serialization budget**: hooks pass `serde_json::Value`. Messages and
  tools are `Serialize + Deserialize`, so round-tripping through JSON is
  the natural boundary. Keep it simple; don't try to avoid the ser/deser.
- **No schema changes to HookResult**: `Pass(serde_json::Value)` is the
  right return shape — the caller extracts what it needs.
- **Greenfield posture**: refactor freely.

## Approach

Three independent fixes, each a commit boundary.

**Fix 1 — Hook context fidelity.** For each hook call-site, serialize the
actual data the brainstorm requires into the JSON context, and after the
hook returns `Pass(ctx)`, deserialize any mutated fields back and use them.
Where the brainstorm says a field is immutable input (e.g., `messages` in
`after_completion` is input, `response` is mutable), only deserialize the
mutable field.

**Fix 2 — TurnComplete persistence.** Append `TurnComplete` to
`self.events` *before* calling `persist_and_compact()`, so it falls within
the slice that gets written to the session provider.

**Fix 3 — Async-capable dispatch.** Wrap `main()` with `#[tokio::main]` so
a tokio runtime exists. Switch `call_tool` and `call_hook` to use
`mlua::Function::call_async` inside `tokio::runtime::Handle::current().block_on()`,
which allows the Lua coroutine to yield into the tokio executor. Add an
integration test with a Lua tool that calls `ur.http.get` against a local
mock server.

## Implementation plan

### Fix 1: Hook context fidelity

- [ ] `before_completion` (session.rs ~253): serialize `messages`, `model`, `settings`, and `tools` into the hook context; on `Pass`, deserialize back `messages` (as `Vec<Message>`), `model` (string), `settings` (as `Vec<ConfigSetting>`), and `tools` (as `Vec<ToolDescriptor>`) and use them for the completion call
- [ ] `after_completion` (session.rs ~294): pass `messages` (read-only), `model` (read-only), and `response` (the `completion.message` serialized); on `Pass`, deserialize `response` back as `Message` and replace `completion.message` before proceeding
- [ ] `after_session_load` (session.rs ~164): pass `session_id` and `messages` (derived from events via `messages_from_events`); on `Pass`, deserialize `messages` and rebuild events if changed
- [ ] `before_session_append` (session.rs ~532): pass `session_id` and the full `event` serialized as JSON; on `Pass`, deserialize the (possibly mutated) event back and use it for storage
- [ ] `before_compaction` (session.rs ~556): pass `messages` (the full serialized message list); on `Pass`, deserialize `messages` back and feed the mutated list to the compaction provider
- [ ] `after_compaction` (session.rs ~584): pass `original_messages` and `compacted` (both serialized); on `Pass`, deserialize `compacted` back as `Vec<Message>` and use the result
- [ ] Update test extension hooks to exercise a mutation: `before_completion` should modify the model field; `after_tool` should append a suffix to the result; verify these are observable in test output

### Fix 2: TurnComplete persistence

- [ ] Move the `TurnComplete` event push (session.rs ~351) to *before* the `persist_and_compact()` call (before line 349)
- [ ] Emit `SessionEvent::TurnComplete` to the callback *after* persistence succeeds (keep the user-facing event after write, just move the internal event before)
- [ ] Add a unit test: after `run_turn`, verify that the last persisted event in storage is `TurnComplete`

### Fix 3: Async-capable Lua dispatch

- [ ] Add `#[tokio::main]` to `fn main()` in `src/main.rs` so a tokio runtime is available for the entire process
- [ ] In `lua_host.rs`, change `call_tool` to use `tokio::runtime::Handle::current().block_on(handler_key.call_async(args))` instead of `handler_key.call(args)`
- [ ] In `lua_host.rs`, change `call_hook` to use the same `block_on(handler_key.call_async(ctx_lua))` pattern
- [ ] Add a Lua tool to the test extension that calls `ur.http.get` and returns the status code — e.g., `ur.tool("http_status", { handler = function(args) local r = ur.http.get(args.url) return tostring(r.status) end })`
- [ ] Add an integration test that invokes the `http_status` tool against a mock HTTP server (using `mockito` or a simple `tokio::net::TcpListener` fixture) and asserts the returned status code is correct

### Finalize

- [ ] Run `make verify` — all checks pass
- [ ] Review that every hook call-site now matches the brainstorm contract table

## Validation

- `before_completion` hook can mutate messages, model, settings, and tools;
  all four are used in the subsequent `stream_completion` call
- `after_completion` hook can mutate `response`; the mutated response is
  what gets pushed to events and displayed
- `before_session_append` hook receives the full event and can mutate it
- `before_compaction` hook receives and can mutate the message list
- `after_compaction` hook receives and can mutate the compacted output
- `TurnComplete` is present in the session provider's stored events
  immediately after `run_turn` returns
- A Lua tool handler that calls `ur.http.get` returns a real HTTP response
- `make verify` passes

## Risks

- **Serialization cost**: passing full message lists through JSON to every
  hook adds overhead. Acceptable for now; optimize later with lazy
  serialization if profiling shows a problem.
- **`after_session_load` mutation semantics**: rebuilding internal events
  from a hook-mutated message list is lossy (events contain more than
  messages). Consider simplifying to observability-only for this hook, or
  documenting that only the message-derived subset is mutable.
- **`block_on` in sync context**: calling `block_on` from within a tokio
  context can panic if the runtime is single-threaded. Using
  `#[tokio::main]` with the default multi-thread scheduler avoids this, but
  test harnesses must also have a runtime.
