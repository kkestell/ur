# Fix review findings: panics, security, correctness, dead code

## Goal

Address findings from two review passes. The first pass (8 items, all done) fixed panics, security holes, and correctness bugs. The second pass (6 items below) removes dead abstractions, unifies duplicated types, and wires up the dormant tool-approval pipeline.

## Related code

- `src/providers/mod.rs` — `LlmProvider` trait (sync, used as `dyn`)
- `src/providers/google.rs` — `GoogleProvider` impl; `block_on` in `complete()` (line 253)
- `src/providers/openrouter.rs` — `OpenRouterProvider` impl; `block_on` in 3 places (lines 252, 257, 270)
- `src/session.rs` — `run_turn` (sync, calls provider methods); turn persistence gap; settings round-trip
- `src/workspace.rs` — holds `Vec<Arc<dyn LlmProvider>>`, passes to session/host_api
- `src/model.rs` — `collect_provider_models` takes `&[&dyn LlmProvider]`
- `src/host_api.rs` — `build_complete_fn` uses `Arc<dyn LlmProvider>`; HTTP module has no timeout
- `src/main.rs` — `#[tokio::main]` async, but `run()` is sync
- `src/lua_host.rs` — `parse_tool_arguments_or_nil` evals args as Lua (line 192); `call_handler` already uses `block_in_place` (line 210)
- `src/providers/session_jsonl.rs` — unsanitized `session_id` in path (line 28)
- `src/logging.rs` — `expect()` panics (lines 54, 61)
- `extensions/workspace/test-extension/init.lua` — `response.content` should be `response.body` (line 51)

## Verified assumptions

1. **Exactly 2 `LlmProvider` impls exist** (Google, OpenRouter). No mocks, no test impls. Enum dispatch is viable.
2. **`dyn LlmProvider` appears in 8 sites across 4 files** (`workspace.rs`, `session.rs`, `host_api.rs`, `model.rs`). All become concrete enum references.
3. **`SessionProvider` and `CompactionProvider` are fine** — sync file I/O, no async needed.
4. **`call_handler` in `lua_host.rs` already uses `block_in_place`** (line 210). The providers should have used this pattern but didn't — we're going async instead.
5. **`build_complete_fn` in `host_api.rs` uses `create_function` (sync)** and calls `list_models()` + `complete()`. Must become `create_async_function`.
6. **TUI does not call LLM methods yet** — no async changes needed there.
7. **No existing tests create LlmProvider instances** — tests use `HostProviders::default()` with empty providers.
8. **Rust 1.93** — RPITIT and async-in-trait are stable, but not needed since we're dropping the trait.

## Approach

### Architecture: enum dispatch replaces dyn trait

Replace `trait LlmProvider` + `Arc<dyn LlmProvider>` with a concrete `LlmProvider` enum:

```rust
pub enum LlmProvider {
    Google(google::GoogleProvider),
    OpenRouter(openrouter::OpenRouterProvider),
}
```

The enum delegates each method to the inner variant. Async methods work directly — no object safety issues, no `async-trait` crate, no `block_on`.

`list_models()` and `list_settings()` become async too (OpenRouter fetches its catalog over HTTP). Google's impls return immediately.

### Fix ordering

1. **Enum dispatch + async** — biggest change, unblocks provider paths
2. **Lua code injection** — one function, surgical
3. **Session path traversal** — one function + defense-in-depth
4. **Turn persistence** — wrap `run_turn` body to catch errors
5. **HTTP timeouts** — shared client with timeout
6. **Startup panics** — `expect` → `Result`
7. **Settings enum round-trip** — tagged serialization
8. **Test extension field name** — one-line fix + test assertion

## Implementation plan

### 1. Replace `LlmProvider` trait with async enum (High — panic)

- [x] Create `pub enum LlmProvider` in `src/providers/mod.rs` with variants `Google(GoogleProvider)`, `OpenRouter(OpenRouterProvider)`
- [x] Implement delegating methods: `provider_id()` (sync), `list_models()` (async), `list_settings()` (async), `complete()` (async)
- [x] Remove the `trait LlmProvider` definition
- [x] In `GoogleProvider`: remove `impl LlmProvider for GoogleProvider` block; rename `stream_completion` to `complete`; keep `list_models`/`list_settings` as inherent async methods that return immediately
- [x] In `OpenRouterProvider`: remove `impl LlmProvider for OpenRouterProvider` block (the three `block_on` wrappers); rename existing async inherent methods if needed
- [x] Update `src/workspace.rs`: change `Vec<Arc<dyn LlmProvider>>` to `Vec<Arc<LlmProvider>>`; update construction in `new()`
- [x] Update `src/session.rs`: change field types; make `run_turn` async; make `stream_completion` async; await provider calls
- [x] Update `src/model.rs`: change `collect_provider_models` to async, take `&[&LlmProvider]` (concrete)
- [x] Update `src/workspace.rs`: make `provider_models()`, `list_roles()`, `resolve_role()`, `set_role()` async
- [x] Update `src/host_api.rs`: change `HostProviders` to use concrete type; change `build_complete_fn` to use `create_async_function`; await `list_models()` and `complete()`
- [x] Update `src/main.rs`: make `run()`, `handle_run()`, `handle_role()` async
- [x] Run `make verify`

### 2. Stop evaluating tool arguments as Lua code (High — injection)

- [x] In `lua_host.rs` `parse_tool_arguments_or_nil`, remove the `lua.load(arguments_json).eval()` path; use only JSON parse → `lua.to_value`
- [x] Run `make verify`

### 3. Sanitize session IDs against path traversal (High — security)

- [x] In `session_jsonl.rs` `session_path`, reject IDs containing path separators or `..`
- [x] Defense in depth: host_api's `build_session_module` calls go through the validated provider
- [x] Add unit tests: `../etc/passwd`, slashes, backslash, empty, null byte all rejected
- [x] Run `make verify`

### 4. Persist TurnInterrupted on failed turns (High — data loss)

- [x] In `run_turn`, capture errors after `TurnStarted`/`UserMessage`; push `TurnInterrupted` and call `persist_and_compact` before returning error
- [x] Emit `SessionEvent::TurnError` to callback
- [x] Run `make verify`

### 5. Add HTTP timeouts for extension network calls (Medium — hang)

- [x] In `host_api.rs` `build_http_module`, create one shared `reqwest::Client` with 30s timeout; pass into closures
- [x] Run `make verify`

### 6. Replace startup panics with error propagation (Medium — panic)

- [x] In `logging.rs` `init`, change two `expect()` to return `Result<LogHandle>`
- [x] Update callers in `main.rs` and `ur-tui/main.rs`
- [x] Run `make verify`

### 7. Preserve SettingValue::Enumeration through hook round-trip (Medium — correctness)

- [x] In `format_setting_value`, serialize `Enumeration` as `{"__enum": value}` (distinct from plain string)
- [x] In `parse_settings_from_json`, detect `{"__enum": ...}` and reconstruct `Enumeration`
- [x] Add unit test: round-trip preserves variant
- [x] Run `make verify`

### 8. Fix test extension field name and test coverage (Medium — correctness)

- [x] In `extensions/workspace/test-extension/init.lua` line 51, change `response.content` to `response.body`
- [x] In `tests/extensions/test_extension.rs`, add assertion that `content_length > 0`
- [x] Run `make verify`

## Validation

- `make verify` after each step
- Smoke test: `UR_ROOT=/tmp/ur-test OPENROUTER_API_KEY=x cargo run --bin ur -- role list` errors gracefully, no panic

## Risks

- **Enum dispatch is a closed set** — adding a third provider later means adding a variant. This is fine for greenfield with 2 providers; if the project grows to many providers, reconsider.
- **Step 1 touches 7 files** — but each change is mechanical (replace type, add `.await`). No logic changes.

---

## Pass 2: simplification and approval pipeline

### Additional related code

- `src/session.rs:55` — `SessionEvent::ApprovalRequired` variant (emitted during replay but never during live turn)
- `src/session.rs:66` — `ApprovalDecision` enum (callback return type, always ignored in dispatch)
- `src/session.rs:484` — `dispatch_tool_calls` — no approval check before executing handler
- `src/session.rs:71` — `PersistedEvent` — mirrors `types::SessionEvent` almost 1:1
- `src/session.rs:916` — `types_event_to_persisted` / `persisted_to_types_event` — mechanical 1:1 conversion
- `src/types.rs:114` — `types::SessionEvent` — serializable duplicate of `session::PersistedEvent`
- `src/types.rs:86` — `ToolChoice` enum — never passed as non-None anywhere
- `src/workspace.rs:23` — `ToolHandler` type alias — flattened closure registry duplicating extension runtime
- `src/session.rs:21` — `ToolHandler` type alias (same, duplicated in session)
- `src/host_api.rs:198` — `complete()` call passes `None` for tool_choice
- `src/providers/google.rs:517` — `ToolChoice` mapping logic (unreachable)
- `src/providers/openrouter.rs:844` — `ToolChoice` mapping logic (unreachable)
- `src/manifest.rs:170` — `hook_order` recomputes order from scratch each call
- `src/session.rs:838` — `dispatch_hook` calls `hook_order` + `run_hook_ordered` every time
- `src/hooks.rs:91` — `run_hook_ordered` rebuilds extension order from ID list
- `src/extension_settings.rs` — vestigial module, `#[allow(dead_code)]`, unused outside its own tests
- `extensions/system/read-file/` — existing system extension with `fs-read` capability (pattern for new `write-file`)

### Verified assumptions (pass 2)

1. **`ApprovalRequired` is never emitted during a live turn.** `dispatch_tool_calls` (line 484) executes every tool handler immediately with no approval gate. The only emission is in `replay()` (line 462) when loading persisted `ToolApprovalRequested` events. The callback's `Option<ApprovalDecision>` return is never inspected in `dispatch_tool_calls`.
2. **`PersistedEvent` and `types::SessionEvent` are near-identical.** Both have the same variants (TurnStarted, UserMessage, LlmCompletion, ToolResult, ToolApprovalRequested/Decided, TurnComplete, TurnInterrupted). The difference: `PersistedEvent` is the in-memory representation, `types::SessionEvent` adds `#[derive(Serialize, Deserialize)]` and `serde(tag = "type")`. Two 40-line functions (`types_event_to_persisted`, `persisted_to_types_event`) do mechanical 1:1 conversion.
3. **`ToolChoice` is always `None`.** `stream_completion` (line 850) does not accept or pass `tool_choice`. `host_api::build_complete_fn` (line 198) passes `None`. Both provider `complete()` methods accept `Option<&ToolChoice>` and have full mapping logic that is unreachable.
4. **`ToolHandler` type alias is defined identically in both `workspace.rs:24` and `session.rs:22`.** The workspace builds the registry in `open_session`, the session stores it. The same extensions are also kept as `Vec<Arc<LuaExtension>>` for hook dispatch.
5. **`hook_order` + `run_hook_ordered` rebuilds extension order from scratch every call.** `manifest::hook_order` iterates the manifest to produce an ID list, then `hooks::run_hook_ordered` does a nested loop to match those IDs back to extension objects. The manifest and extensions don't change mid-session.
6. **`extension_settings.rs` has no callers.** Only referenced via `pub mod extension_settings` in `lib.rs`. Its sole function `glob_match` is `#[allow(dead_code)]`.
7. **`ur.fs.write` already exists** behind the `fs-write` capability (host_api.rs line 333). A `write-file` system extension just needs to wrap it like `read-file` wraps `ur.fs.read`.

### Approach (pass 2)

#### Wire up the tool-approval pipeline (High)

The types, callback signature, and persistence variants are all in place. The missing piece is: `dispatch_tool_calls` should emit `ApprovalRequired`, wait for the callback's `ApprovalDecision`, persist the request/decision, and skip execution on `Deny`. The extension capability `fs-write` is a natural trigger — tools from extensions with write capabilities should require approval. Add a `write-file` system extension (mirroring `read-file`) as a concrete tool that exercises the approval path.

#### Unify session event types (High)

Delete `PersistedEvent` entirely. Use `types::SessionEvent` as the single event representation throughout. The JSONL provider already serializes `types::SessionEvent`; the session layer can use it directly. This removes `types_event_to_persisted`, `persisted_to_types_event`, and the duplicated `ApprovalDecision` enum in `session.rs`.

#### Collapse the tool handler registry (Medium)

`dispatch_tool_calls` should call through the extensions directly instead of through the intermediate `Vec<ToolHandler>` closure registry. The session already holds `Vec<Arc<LuaExtension>>`. Each `LuaExtension` already has `tool_descriptors()` and `call_tool()`. The `ToolHandler` type alias and the registry-building loop in `workspace::open_session` become unnecessary.

#### Remove dead `ToolChoice` code (Medium)

Delete the `ToolChoice` enum from `types.rs`. Remove the `tool_choice` parameter from both providers' `complete()` methods and the corresponding mapping logic. If/when tool-choice support is needed, it can be added back with a real call site.

#### Cache hook dispatch order (Low)

Pre-compute the ordered extension list per hook point once during session construction and store it. `dispatch_hook` then uses the cached order directly instead of calling `manifest::hook_order` + rebuilding on every invocation.

#### Delete `extension_settings.rs` (Low)

Remove the module and its `pub mod` declaration in `lib.rs`.

### Implementation plan (pass 2)

### 9. Wire up tool-approval pipeline + write-file extension (High — correctness)

- [x] Add `requires_approval` field to `ToolDescriptor` (default `false`); set based on `fs-write` or `network` capabilities
- [x] In `dispatch_tool_calls`, emit `ApprovalRequired`, persist request/decision, skip on deny
- [x] Create `extensions/system/write-file/extension.toml` with `capabilities = ["fs-write"]`
- [x] Create `extensions/system/write-file/init.lua`
- [x] Run `make verify`

### 10. Unify session event types (High — simplification)

- [x] Remove `PersistedEvent` enum from `session.rs`
- [x] Remove `ApprovalDecision` enum from `session.rs` (use `types::ApprovalDecision` everywhere)
- [x] Replace all `PersistedEvent` usage in `UrSession` fields and methods with `types::SessionEvent`
- [x] Delete `types_event_to_persisted` and `persisted_to_types_event`
- [x] Update `messages_from_events` to match on `types::SessionEvent` variants
- [x] Update `replay()` to use `types::SessionEvent` directly
- [x] Run `make verify`

### 11. Collapse tool handler registry (Medium — simplification)

- [x] Remove `ToolHandler` type alias from `session.rs` and `workspace.rs`
- [x] Remove `tool_handlers` field from `UrSession` and `SessionDeps`
- [x] Remove the registry-building loop in `workspace::open_session`
- [x] In `dispatch_tool_calls`, look up the tool by name via `self.extensions.iter().find(ext has tool)` and call `ext.call_tool(&name, &args)` directly
- [x] Collect `ToolDescriptor` lists from `self.extensions` directly where needed (tool list for LLM)
- [x] Run `make verify`

### 12. Remove dead `ToolChoice` code (Medium — dead code)

- [x] ~~Delete `ToolChoice`~~ — wired up instead: added serde derives, threaded through `stream_completion`, exposed in `before_completion` hook context, and `ur.complete()` opts
- [x] Run `make verify`

### 13. Cache hook dispatch order (Low — efficiency)

- [x] Add `hook_cache: HashMap<HookPoint, Vec<Arc<LuaExtension>>>` to `UrSession`
- [x] Populate it in `UrSession::open` via `build_hook_cache`
- [x] Add `dispatch_hook_cached` that uses pre-ordered cache; keep `dispatch_hook` for pre-construction calls in `open()`
- [x] Run `make verify`

### 14. Delete vestigial `extension_settings.rs` (Low — dead code)

- [x] Remove `pub mod extension_settings;` from `src/lib.rs`
- [x] Delete `src/extension_settings.rs`
- [x] Run `make verify`

## Validation

- `make verify` after each step
- Smoke test: `UR_ROOT=/tmp/ur-test OPENROUTER_API_KEY=x cargo run --bin ur -- role list` errors gracefully, no panic

## Risks

- **Enum dispatch is a closed set** — adding a third provider later means adding a variant. This is fine for greenfield with 2 providers; if the project grows to many providers, reconsider.
- **Step 1 touches 7 files** — but each change is mechanical (replace type, add `.await`). No logic changes.
- **Step 10 changes serialization format** — existing JSONL session files already use `types::SessionEvent` on disk, so no migration needed. The change is purely in-memory representation.
- **Step 9 changes `ToolDescriptor`** — adding `requires_approval` field. Since `ToolDescriptor` derives `Serialize, Deserialize`, the new field needs `#[serde(default)]` to avoid breaking existing serialized tool schemas.

## Out of scope

- Docs drift in `docs/development/extensions/index.md`
- Empty CLI test suites in `tests/cli/{google,openrouter,role}.rs`
