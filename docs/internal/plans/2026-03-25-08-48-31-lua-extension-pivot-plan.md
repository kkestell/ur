# Lua Extension Pivot

## Goal

Replace the WASM Component Model extension system with embedded Luau (via mlua),
pull LLM providers / session storage / compaction into native Rust, and ship a
working Lua extension that exercises tools + all 9 lifecycle hooks.

## Desired outcome

- Zero WASM infrastructure remains (wasmtime, WIT, wit-bindgen, wasm32-wasip2
  targets, extension Cargo.tomls)
- LLM providers (Google, OpenRouter), session storage (JSONL), and compaction
  are native Rust modules in the host
- Extensions are directory-based (`extension.toml` + `init.lua`) discovered
  across the existing 3-tier system
- Each extension runs in its own sandboxed Luau VM with capability-gated host
  APIs
- One test Lua extension validates the full surface (tools + all 9 hooks)
- `make verify` passes with the new system

## Related documents

- `docs/internal/brainstorms/2026-03-25-08-46-11-lua-extension-pivot-brainstorm.md`
- `docs/internal/brainstorms/2026-03-23-16-15-50-react-loop-hooks-brainstorm.md`

## Related code

- `src/extension_host.rs` — wasmtime loading, 4 world variants, capability enforcement (removed)
- `src/slot.rs` — slot definitions, cardinality rules (removed)
- `src/discovery.rs` — 3-tier WASM discovery, checksum, slot detection (rewritten for Lua)
- `src/manifest.rs` — workspace manifest, merge/enable/disable with slot semantics (simplified, no slots)
- `src/extension_settings.rs` — extension init with settings, API key handling (rewritten)
- `src/session.rs` — turn state machine, delegates to providers (refactored to call native providers + hooks)
- `src/workspace.rs` — coordinator, extension queries, role mgmt (adapted)
- `src/config.rs` — role resolution, extension settings storage (adapted)
- `src/model.rs` — provider model catalog (adapted for native providers)
- `src/provider.rs` — API key resolution helper (kept)
- `src/app.rs` — owns wasmtime engine (engine removed, Lua VMs managed differently)
- `src/cli.rs` — clap CLI structures (adapted)
- `src/main.rs` — command handlers (adapted)
- `src/lib.rs` — module declarations (updated)
- `wit/world.wit` — WIT interface definitions (removed)
- `wit/deps/` — WASI wit deps (removed)
- `extensions/system/llm-google/` — Google Gemini WASM extension (removed, logic moves to native module)
- `extensions/system/llm-openrouter/` — OpenRouter WASM extension (removed, logic moves to native module)
- `extensions/system/session-jsonl/` — JSONL session WASM extension (removed, logic moves to native module)
- `extensions/system/compaction-llm/` — compaction WASM extension (removed, logic moves to native module)
- `extensions/workspace/test-extension/` — WASM test extension (removed, replaced by Lua test extension)
- `extensions/workspace/llm-test/` — WASM test LLM provider (removed, replaced by native test harness)
- `Cargo.toml` — wasmtime deps removed, mlua added
- `Makefile` — WASM targets removed, Lua extension handling added
- `tests/cli/` — integration tests (rewritten for new extension model)

## Current state

- Extension system is fully WASM-based: wasmtime 43, WIT 0.4.0, 4 world variants
- Three required slots with cardinality enforcement: `llm-provider` (AtLeastOne), `session-provider` (ExactlyOne), `compaction-provider` (ExactlyOne)
- Four built-in WASM extensions: llm-google, llm-openrouter, session-jsonl, compaction-llm
- Two test WASM extensions: test-extension, llm-test
- No lifecycle hooks implemented yet (brainstormed but not built)
- 30+ integration tests in `tests/cli/` covering discovery, enable/disable, config, roles
- Host types (`message`, `tool-call`, `session-event`, etc.) defined in WIT and must move to native Rust types

## Constraints

- **Clean break** — no WASM coexistence or migration period
- **Greenfield posture** — refactor freely, no backwards compat
- **Scope boundary** — system tools (bash, read_file, etc.) are out of scope; they will become Lua extensions later but not in this pivot
- **One VM per extension** — complete state isolation, sandboxed per declared capabilities
- **mlua feature set** — `luau`, `async`, `send`, `serde`, `macros`

## Research

### Repo findings

- `extension_host.rs` (672 lines) is the single largest removal — it owns wasmtime engine config, 4 bindgen! worlds, `ExtensionInstance` enum with dispatch macros, `HostState` implementing `WasiView` + `WasiHttpView`, capability validation
- `slot.rs` defines 3 slots with `SLOT_EXPORTS` mapping WIT export names to slot names — entire concept goes away since providers become native
- `discovery.rs` already implements the 3-tier scan pattern (system/user/workspace) — the scan logic is reusable but switches from finding `.wasm` files to finding `extension.toml` files
- `manifest.rs` (517 lines) has slot-aware merge/enable/disable with exactly-1 switch semantics — simplifies dramatically without slots, but the core manifest persistence and merge patterns remain useful
- `session.rs` (37KB) is the turn state machine — currently delegates to extension instances for LLM completion, session persistence, and compaction. These delegate calls change from WASM calls to native Rust calls
- LLM provider logic in `llm-google` and `llm-openrouter` is substantial (~400-500 lines each) with streaming, model catalogs, per-model settings, and API integration — this moves to native Rust modules essentially unchanged
- Session JSONL logic is straightforward file I/O with serde — moves to native with minimal changes
- Compaction is currently a stub (returns messages unchanged) — moves to native as-is
- `config.rs` role resolution parses "provider/model" references — works the same with native providers
- API key resolution in `provider.rs` (env var > keyring > empty) is provider-agnostic and stays

### External research: mlua

- **Version:** 0.11.6 stable. Features needed: `luau`, `async`, `send`, `serde`, `macros`
- **Sandboxing:** `Lua::sandbox(true)` freezes stdlib, isolates globals per thread, restricts `loadstring` to text-only. Extensions can only call APIs the host explicitly registers
- **Async:** `create_async_function` wraps Rust futures in Lua coroutines. `Poll::Pending` triggers `coroutine.yield()`, executor resumes when ready. Works with tokio
- **Resource control:** `set_memory_limit(bytes)` for per-VM memory caps. `set_interrupt` fires at every function call and loop iteration for execution timeouts. `set_memory_category` for per-extension allocation tracking
- **Host API registration:** `UserData` trait for Rust types, table-of-functions for module-style APIs, `set_app_data` for shared state accessible from callbacks
- **`send` feature:** makes `Lua: Send + Sync` via `Arc` + `parking_lot::ReentrantMutex`. Required for multi-threaded tokio. All callbacks must be `Send`
- **Custom `require`:** Luau-specific `Require` trait lets us control module resolution — extensions `require("ur")` gets the host-injected API table
- **Limitation:** async metamethods not available in Luau. Not a blocker — we don't need them

## Assumptions to validate

- **mlua async + tokio multi-thread works reliably with `send`**
  - Why it matters: the host uses tokio; if async Lua calls are flaky, the entire hook/tool system is unreliable
  - How to confirm: spike in task 1 — create a sandboxed VM, register an async function, call it from Lua on tokio
- **Luau sandbox + custom `require` is sufficient to prevent escape**
  - Why it matters: extensions must not access host state except through registered APIs
  - How to confirm: spike in task 1 — verify sandboxed code cannot access `io`, `os`, `debug`, `loadfile`, or raw `ffi`
- **Per-VM memory limits work under concurrent load**
  - Why it matters: one runaway extension must not OOM the host
  - How to confirm: spike in task 1 — set a 10MB limit, allocate beyond it, verify `MemoryError` is raised

## Open questions

- **Extension config delivery:** The brainstorm suggests `ur.config` populated before `init.lua` runs. This aligns with the current `init(config)` pattern. Carry forward as a config table set on the `ur` module before `dofile("init.lua")`
- **Error handling:** When a tool handler errors, the LLM sees a `ToolResult` with error text (consistent with the hooks brainstorm Option A). Lua errors in handlers are caught and surfaced as tool error results
- **Hot reload:** Out of scope for this pivot. Design for it by keeping VM lifecycle simple (create/destroy, no long-lived state assumptions)
- **Luau type stubs:** Nice to have, not blocking. Can ship a `ur.d.luau` later

## Approach

The pivot is a **replacement, not a migration**. The work is structured in 6 phases:

1. **Spike** — validate mlua+Luau assumptions before committing
2. **Remove** — delete all WASM infrastructure
3. **Native providers** — move provider logic into host Rust modules
4. **Lua runtime** — build the new extension host, discovery, manifest, host API
5. **Hooks** — implement the 9 lifecycle hooks in the session turn loop
6. **Validate** — test extension, integration tests, `make verify`

Each phase is a commit boundary. Phases 2-3 will temporarily break things (no
extensions work). Phase 4 restores extension functionality. Phase 5 adds new
capability. Phase 6 proves it all works.

## Implementation plan

### Phase 1: Spike — validate mlua assumptions

- [ ] Add `mlua` dependency: `cargo add mlua --features luau,async,send,serde,macros`
- [ ] Create `src/lua_spike.rs` (temporary) that validates:
  - Sandboxed VM creation with `Lua::sandbox(true)`
  - Custom `require("ur")` returning a host-injected table
  - Registering a sync Rust function callable from Lua
  - Registering an async Rust function callable from Lua (on tokio runtime)
  - Memory limit enforcement (`set_memory_limit`)
  - Interrupt-based execution timeout (`set_interrupt`)
  - Serde round-trip of Lua tables to/from Rust types
- [ ] Run spike as a test; confirm all assertions pass
- [ ] Delete `src/lua_spike.rs` after validation

### Phase 2: Remove WASM infrastructure

- [ ] Delete `wit/` directory (world.wit + deps/)
- [ ] Delete `extensions/system/llm-google/` (entire crate)
- [ ] Delete `extensions/system/llm-openrouter/` (entire crate)
- [ ] Delete `extensions/system/session-jsonl/` (entire crate)
- [ ] Delete `extensions/system/compaction-llm/` (entire crate)
- [ ] Delete `extensions/workspace/test-extension/` (entire crate)
- [ ] Delete `extensions/workspace/llm-test/` (entire crate)
- [ ] Remove `wasmtime`, `wasmtime-wasi`, `wasmtime-wasi-http` from Cargo.toml: `cargo remove wasmtime wasmtime-wasi wasmtime-wasi-http`
- [ ] Remove `sha2` if only used for WASM checksums (check first)
- [ ] Delete `src/extension_host.rs`
- [ ] Delete `src/slot.rs`
- [ ] Strip wasmtime engine from `src/app.rs` (`UrApp` no longer owns an engine)
- [ ] Update `src/lib.rs` — remove `extension_host`, `slot` module declarations
- [ ] Update Makefile — remove `build-extensions`, `WASM_TARGET`, `BUILTIN_EXTENSION_MANIFESTS`, `TEST_EXTENSION_MANIFESTS`, wasm-related install logic
- [ ] Ensure `cargo check` passes (will have dead code / missing references — fix compile errors in workspace.rs, session.rs, main.rs, etc. by stubbing or commenting out extension-dependent code paths temporarily)

### Phase 3: Native Rust providers

- [ ] Define native Rust types for shared domain model (replaces WIT types):
  - `Message`, `MessagePart` (text, tool-call, tool-result), `Usage`
  - `ToolCall`, `ToolResult`, `ToolDescriptor`, `ToolChoice`
  - `SessionInfo`, `SessionEvent` (9 variants matching WIT)
  - `CompletionChunk`, `ModelDescriptor`
  - `SettingDescriptor`, `SettingSchema`, `SettingValue`
  - Put these in a new `src/types.rs` module
- [ ] Create `src/providers/mod.rs` with submodules
- [ ] Create `src/providers/google.rs` — port `llm-google` extension logic to native Rust:
  - Model catalog (gemini-3-flash-preview, gemini-3.1-pro-preview, gemini-3.1-flash-lite-preview)
  - Per-model settings (thinking_level, max_output_tokens, etc.)
  - Streaming completion via reqwest/hyper (replaces WASI HTTP)
  - API key from config
- [ ] Create `src/providers/openrouter.rs` — port `llm-openrouter` extension logic to native Rust:
  - Dynamic catalog from OpenRouter API
  - Per-model dynamic settings
  - Streaming completion
  - API key from config
- [ ] Create `src/providers/session_jsonl.rs` — port `session-jsonl` logic to native Rust:
  - JSONL file I/O using std::fs (replaces WASI filesystem)
  - Session directory: `~/.ur/sessions/` (or configurable)
  - `load`, `append`, `list` operations
- [ ] Create `src/providers/compaction.rs` — port `compaction-llm` to native Rust:
  - Currently a stub (returns messages unchanged) — keep as stub
  - Will later use an LLM provider for actual compaction
- [ ] Define provider traits: `LlmProvider`, `SessionProvider`, `CompactionProvider`
- [ ] Update `src/model.rs` to build catalogs from native `LlmProvider` instances instead of WASM extensions
- [ ] Update `src/session.rs` to call native provider traits instead of `ExtensionInstance` methods
- [ ] Update `src/workspace.rs` to instantiate and manage native providers
- [ ] Update `src/config.rs` role resolution to work with native provider catalogs
- [ ] Adapt `src/extension_settings.rs` for native provider settings (API keys, per-model config)
- [ ] Ensure `cargo check` passes and the core turn loop works without extensions

### Phase 4: Lua extension runtime

- [ ] Create `src/lua_host.rs` — the Lua extension runtime:
  - `LuaExtension` struct: owns `mlua::Lua` instance, extension metadata
  - `load(path: &Path) -> Result<LuaExtension>` — reads `extension.toml`, creates sandboxed VM, injects `ur` module, executes `init.lua`
  - Per-VM resource limits: `set_memory_limit`, `set_interrupt` for timeout
  - Capability enforcement: only register gated APIs if extension declares the capability
- [ ] Create `src/host_api.rs` — the `ur` module exposed to Lua:
  - `ur.log(msg)` — always available, routes to tracing
  - `ur.tool(name, spec)` — registers a tool (name, description, parameters, handler fn)
  - `ur.hook(name, fn)` — registers a lifecycle hook handler
  - `ur.config` — table populated from user config before init
  - `ur.complete(messages, opts)` — async, calls native LLM provider (never triggers hooks — the `complete-raw` concept)
  - `ur.session.load(id)` / `ur.session.list()` — read-only session access
  - `ur.http.get(url, opts)` / `ur.http.post(url, body, opts)` — gated on `network` capability
  - `ur.fs.read(path)` / `ur.fs.write(path, content)` / `ur.fs.list(path)` — gated on `fs-read` / `fs-write`
- [ ] Create `src/extension_manifest.rs` — parse `extension.toml`:
  - `[extension]` table: `id`, `name`, `capabilities` (list of strings)
  - Validate capability strings against known set: `network`, `fs-read`, `fs-write`
- [ ] Rewrite `src/discovery.rs`:
  - Scan 3-tier directories for subdirectories containing `extension.toml`
  - Parse manifest, record id/name/capabilities/source-tier/path
  - No Lua execution during discovery
  - `DiscoveredExtension` struct updated: no wasm_path, no checksum, no slot; add `dir_path`
- [ ] Simplify `src/manifest.rs`:
  - Remove all slot-related logic (cardinality, switch semantics, slot validation)
  - Keep: workspace manifest persistence, merge logic (new extensions default state), enable/disable
  - Add: hook ordering storage per hook-point
- [ ] Update `src/workspace.rs`:
  - Load Lua extensions via `lua_host::load()` for all enabled discovered extensions
  - Collect registered tools across all extensions
  - Provide tool dispatch: route `call_tool(name, args)` to the owning extension's handler
- [ ] Update `src/lib.rs` — add new module declarations: `lua_host`, `host_api`, `extension_manifest`, `types`, `providers`
- [ ] Update `src/main.rs` and `src/cli.rs`:
  - `extension list` — show Lua extensions (id, name, enabled, source tier)
  - `extension enable/disable` — no slot semantics, simple toggle
  - `extension inspect` — show capabilities, registered tools, registered hooks
  - Remove config list/get/set for extension settings (extensions configure via `ur.config` from user config, not the old per-setting WIT model) — or adapt if there's still a need

### Phase 5: Lifecycle hooks

- [ ] Define hook dispatch in `src/lua_host.rs`:
  - `HookPoint` enum: 9 variants matching the brainstorm
  - `HookResult` enum: `Pass`, `Modified(Value)`, `Rejected(String)`
  - `run_hook(point, context) -> HookResult` — iterates extensions in manifest-defined order, chains modifications, short-circuits on reject
- [ ] Integrate hooks into `src/session.rs` turn loop:
  - `before_completion` / `after_completion` — wrap LLM provider calls
  - `before_tool` / `after_tool` — wrap tool dispatch
  - `before_session_load` / `after_session_load` — wrap session load
  - `before_session_append` — wrap session append
  - `before_compaction` / `after_compaction` — wrap compaction
- [ ] Hook ordering in manifest:
  - Default order: discovery order (system > user > workspace)
  - Persisted per hook-point in workspace manifest
  - New extensions appended to end of each hook chain
  - Disabled extensions preserved in position, skipped at runtime
- [ ] Rejection semantics: `before_tool` rejection returns a `ToolResult` with error text so the LLM can reason about it. `before_completion` rejection aborts the completion attempt with an error surfaced to the user

### Phase 6: Test extension and integration tests

- [ ] Create test Lua extension at `extensions/workspace/test-extension/`:
  - `extension.toml` with `id = "test-extension"`, `capabilities = ["network"]`
  - `init.lua` registering:
    - A tool (`echo`) that returns its input
    - All 9 lifecycle hooks with observable side effects (e.g., logging, modifying context)
- [ ] Rewrite `tests/cli/extension.rs`:
  - Discovery: finds Lua extensions across tiers
  - Enable/disable: simple toggle without slot semantics
  - Inspect: shows tools and hooks
  - Tool invocation: `echo` tool works end-to-end
- [ ] Rewrite `tests/cli/google.rs` and `tests/cli/openrouter.rs` for native providers
- [ ] Update `tests/cli/role.rs` for native provider model catalogs
- [ ] Update `tests/cli/run.rs` for the new turn loop with hooks
- [ ] Update Makefile:
  - `build` target: just `cargo build` (no extension build step)
  - `test` target: just `cargo test` (no extension pre-build)
  - `install` target: copy binaries only (no WASM extension install)
  - `check`, `clippy`, `fmt`, `fmt-check`: host-only (no extension manifests loop)
- [ ] Run `make verify` — all checks pass

## Validation

- `cargo check` passes after each phase
- `cargo test` passes after phases 1, 3, 6
- `cargo clippy -- -D warnings` clean after phase 6
- `cargo fmt --check` clean throughout
- `make verify` passes at the end
- Test extension exercises all 9 hook points and tool dispatch
- Native providers (Google, OpenRouter) complete streaming requests
- Session JSONL persistence works end-to-end
- Extension discovery finds extensions across all 3 tiers
- Capability enforcement: gated APIs error without declared capability
- Resource limits: memory-limited VM raises error on overallocation

## Risks and follow-up

- **Risk:** Porting LLM provider streaming from WASI HTTP to native reqwest/hyper may surface differences in chunked transfer handling. Mitigate by testing streaming early in phase 3
- **Risk:** `mlua` `send` feature adds overhead from `Arc` + mutex. Profile after phase 4 to ensure hook dispatch latency is acceptable
- **Risk:** Luau sandbox may not prevent all resource exhaustion vectors. Mitigate with `set_memory_limit` + `set_interrupt` in phase 4, but adversarial testing is a follow-up
- **Follow-up:** Ship `ur.d.luau` type stubs for extension author autocomplete
- **Follow-up:** Hot reload for development (watch extension files, recreate VM on change)
- **Follow-up:** Migrate system tools (bash, read_file, write_file, edit_file) to Lua extensions
- **Follow-up:** Extension ordering CLI commands (`ur extension reorder <hook-point>`)
- **Follow-up:** Extension marketplace / install-from-URL
