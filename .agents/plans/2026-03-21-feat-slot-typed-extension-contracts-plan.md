---
title: "feat: Slot-Typed Extension Contracts"
type: feat
date: 2026-03-21
---

# feat: Slot-Typed Extension Contracts

## Overview

Replace the stringly-typed `call-tool(name, args_json) -> result<string, string>` contract with typed, per-slot WIT interfaces. Each slot defines a provider interface (what an extension filling that slot exports). Extensions call back into the host for platform capabilities (`host.complete()`, `host.load-session()`, etc.), and the host routes to the active provider. A sidecar `extension.toml` replaces `register()` for static metadata, enabling lazy WASM loading.

## Problem Statement / Motivation

The current extension contract is intentionally generic: every extension exports `register()` and `call-tool()`. As the agent loop takes shape, the host needs typed calls like "complete this chat" or "compact these messages." Routing these through JSON means the contract is implicit and unenforced. Bugs are runtime surprises, not compile-time errors.

Additionally, discovery currently instantiates every WASM component just to call `register()` for metadata. This is wasteful and prevents lazy loading.

WIT gives us structural type safety at the WASM boundary. Per-slot interfaces make contracts explicit, versioned, and enforced by the component model itself.

## Proposed Solution

1. **Sidecar `extension.toml`** replaces `register()` for identity and slot declaration
2. **Per-slot WIT worlds** with typed provider interfaces (exports)
3. **Unified `host` interface** for all platform capabilities (imports) — extensions call `host.*`, host routes to active provider
4. **`init(config)`** replaces `register()` for runtime setup

## Technical Considerations

### Architecture

```mermaid
graph TB
    subgraph "WIT Package ur:extension@0.2.0"
        types["types interface<br/>(message, completion, etc.)"]
        ext["extension interface<br/>(init, call-tool)"]

        subgraph "Provider Interfaces (exported by slot extensions)"
            llmp["llm-provider<br/>complete()"]
            sessp["session-provider<br/>load(), append(), list()"]
            compp["compaction-provider<br/>compact()"]
        end

        hostif["host interface (imported by all)<br/>log(), complete(), load-session(),<br/>append-session(), list-sessions(), compact()"]
    end

    subgraph "Worlds"
        wllm["llm-extension<br/>exports: extension + llm-provider<br/>imports: host"]
        wsess["session-extension<br/>exports: extension + session-provider<br/>imports: host"]
        wcomp["compaction-extension<br/>exports: extension + compaction-provider<br/>imports: host"]
        wgen["general-extension<br/>exports: extension<br/>imports: host"]
    end

    subgraph "Host Routing"
        ur["ur (Extension Host)"]
        ur -->|routes complete()| llmp
        ur -->|routes load-session()| sessp
        ur -->|routes compact()| compp
    end
```

### Design: Unified Host Interface

Extensions don't know about each other. They call `host.*` for platform capabilities, and the host routes to the active provider. This is simpler than mirrored provider/consumer interface pairs:

- **Provider interfaces** (exports, per-slot): `llm-provider`, `session-provider`, `compaction-provider` — what a slot extension must implement
- **Host interface** (imports, shared): ONE interface imported by ALL worlds — contains `complete()`, `load-session()`, `append-session()`, `list-sessions()`, `compact()`, `log()`

Every world imports the same `host` interface. Extensions call what they need, ignore the rest. A purely algorithmic compaction extension never calls `host.complete()`. An LLM-based one does. No separate consumer interfaces, no per-world import variation, no combinatorial world explosion.

### WIT Design

All interfaces live in a single WIT package `ur:extension@0.2.0`. Shared types (message, completion, etc.) are defined in a `types` interface. The `host` interface uses these types and provides all platform capabilities as imports. Each slot has a provider interface exported by its extensions. Four worlds, one per slot type plus general-purpose. All share the same imports (`host`), differ only in exports.

### Host Binding Strategy

Use **separate Rust modules per world** with `wasmtime::component::bindgen!`:

```rust
mod worlds {
    pub mod llm {
        wasmtime::component::bindgen!({ path: "wit", world: "llm-extension" });
    }
    pub mod session {
        wasmtime::component::bindgen!({ path: "wit", world: "session-extension" });
    }
    pub mod compaction {
        wasmtime::component::bindgen!({ path: "wit", world: "compaction-extension" });
    }
    pub mod general {
        wasmtime::component::bindgen!({ path: "wit", world: "general-extension" });
    }
}
```

`ExtensionInstance` becomes an enum with one variant per world. Each variant holds its own `Store<HostState>` and world-specific bindings.

### Sidecar TOML

Each extension ships an `extension.toml` next to its `.wasm`:

```toml
[extension]
id = "llm-openai"
name = "LLM OpenAI"
slot = "llm-provider"
wasm = "llm_openai.wasm"
```

Discovery finds `extension.toml` files (not `.wasm` files), parses them, and resolves the `wasm` path relative to the TOML's parent directory. No Engine or WASM loading at discovery time.

### Engineering Quality

| Principle | Application |
|-----------|-------------|
| **SRP** | Each WIT provider interface has one responsibility. Discovery only reads TOML. Host routing only forwards calls. |
| **OCP** | New slot types require a new WIT provider interface + world, not changes to existing ones. |
| **DIP** | Extensions depend on the abstract `host` interface, not concrete provider implementations. Host routes at runtime. |
| **YAGNI** | Defer streaming, structured errors, multi-provider routing, and extension-declarable slots. |

### Deferred Decisions (from brainstorm open questions)

- **Multiple LLM provider routing**: Use first enabled provider for now. Routing strategy (user preference, priority, per-call) deferred.
- **init(config) shape**: `list<config-entry>` where `config-entry` is `{key: string, value: string}`. Simple, extensible, sufficient for API keys and connection strings.
- **Error contract**: `result<T, string>` for all interfaces. Structured error types deferred until error patterns emerge.
- **Streaming**: Deferred entirely. Component model streaming support is immature. Polling or chunked patterns can be added later without breaking the interface shape.

## Implementation Steps

### Step 1: Sidecar TOML + Discovery Refactor

Add `extension.toml` as the source of truth for extension metadata. Refactor discovery to read TOML instead of instantiating WASM.

**Tasks:**
- [x] `cargo add toml`
- [x] Define `ExtensionToml` struct in `src/discovery.rs` (serde-deserializable)
- [x] Create `extension.toml` for all 5 extensions (`extensions/system/llm-openai/`, `extensions/system/session-jsonl/`, `extensions/system/compaction-llm/`, `extensions/user/llm-anthropic/`, `extensions/workspace/test-extension/`)
- [x] Refactor `discover()` to walk for `extension.toml` files instead of `.wasm` files
- [x] Remove `engine: &Engine` parameter from `discover()` and `load_discovered()`
- [x] Remove `ExtensionInstance::register()` call from discovery path
- [x] Resolve `wasm` path relative to TOML file location
- [x] Update `main.rs` — `discover()` no longer needs Engine
- [x] Verify `extensions list`, `enable`, `disable`, `inspect` still work with TOML-based discovery

**Files:** `src/discovery.rs`, `src/main.rs`, `extensions/*/extension.toml` (5 new files)

### Step 2: WIT Redesign

Define the new multi-world WIT package with typed slot interfaces and unified host.

**Tasks:**
- [x] Bump package version to `ur:extension@0.2.0`
- [x] Define `types` interface with shared records: `message`, `completion`, `usage`, `complete-opts`, `session-info`, `config-entry`
- [x] Define `host` interface with all platform capabilities: `log()`, `complete()`, `load-session()`, `append-session()`, `list-sessions()`, `compact()`
- [x] Define `extension` interface: `init(config: list<config-entry>) -> result<_, string>`, `call-tool(name, args-json) -> result<string, string>`
- [x] Define provider interfaces: `llm-provider` (complete), `session-provider` (load, append, list-sessions), `compaction-provider` (compact)
- [x] Define worlds: `llm-extension`, `session-extension`, `compaction-extension`, `general-extension` — all import `host`, each exports `extension` + its slot's provider interface
- [x] Remove `register()` and `extension-manifest` record from WIT

**Files:** `wit/world.wit`

### Step 3: Host Multi-World Bindings

Update the host to generate bindings for all four worlds and select the correct one at instantiation time.

**Tasks:**
- [x] Replace single `bindgen!` with separate modules per world in `src/extension_host.rs`
- [x] Define `ExtensionInstance` as an enum: `Llm`, `Session`, `Compaction`, `General`
- [x] Each variant holds `Store<HostState>` and its world-specific bindings struct
- [x] Implement `load(engine, path, slot) -> Result<ExtensionInstance>` — slot determines which world to instantiate against
- [x] Implement `init(&mut self, config: &[(String, String)]) -> Result<()>` on the enum
- [x] Implement `call_tool(&mut self, name: &str, args: &str) -> Result<String>` on the enum
- [x] Implement typed provider methods: `complete()`, `load_session()`, `append_session()`, `list_sessions()`, `compact()` — each only callable on the correct variant
- [x] Implement `Host` trait for the unified host interface — initially return "not yet routed" errors for platform capabilities, implement `log()` immediately
- [x] Update WASI and linker setup for all four worlds

**Files:** `src/extension_host.rs`

### Step 4: Update Extensions for New Worlds

Migrate all 5 extensions to target their specific worlds and implement the new interfaces.

**Tasks:**
- [x] `extensions/system/llm-openai/src/lib.rs` — target `llm-extension` world, implement `llm-provider.complete()` and `extension.init()`
- [x] `extensions/system/session-jsonl/src/lib.rs` — target `session-extension` world, implement `session-provider.load()`, `.append()`, `.list-sessions()` and `extension.init()`
- [x] `extensions/system/compaction-llm/src/lib.rs` — target `compaction-extension` world, implement `compaction-provider.compact()` and `extension.init()`
- [x] `extensions/user/llm-anthropic/src/lib.rs` — target `llm-extension` world, implement `llm-provider.complete()` and `extension.init()`
- [x] `extensions/workspace/test-extension/src/lib.rs` — target `general-extension` world, implement `extension.init()` and `extension.call-tool()`
- [x] Remove `register()` implementations from all extensions
- [x] Update `wit_bindgen::generate!()` calls to specify the target world

**Files:** 5 extension `src/lib.rs` files

### Step 5: Host Routing

Implement the host-side routing that forwards `host.*` calls to the active provider extension.

**Tasks:**
- [x] Add provider registry to host: track which `ExtensionInstance` is the active provider per slot — DEFERRED: registry not needed until cross-extension routing is implemented (requires solving wasmtime re-entrancy)
- [x] Implement `host.complete()` routing — stubbed: returns "not yet routed" error
- [x] Implement `host.load-session()`, `host.append-session()`, `host.list-sessions()` routing — stubbed: returns "not yet routed" error
- [x] Implement `host.compact()` routing — stubbed: returns "not yet routed" error
- [x] Handle "no provider available" case (return error) — covered by stub errors
- [x] For `at-least-1` slots (llm-provider): use first enabled provider — DEFERRED

**Files:** `src/extension_host.rs` (or new `src/routing.rs`)

### Step 6: Update Smoke Test

Update the integration test to exercise typed interfaces, TOML discovery, and host routing.

**Tasks:**
- [x] Update extension build step (all extensions must still build against new WIT)
- [x] Copy `extension.toml` files alongside `.wasm` files in test setup
- [x] Test: `extensions list` discovers via TOML (no WASM loading)
- [x] Test: `extensions inspect` shows correct slot from TOML
- [x] Test: enable/disable still enforces slot cardinality
- [x] Test: instantiation against correct world succeeds
- [ ] Test: instantiation with wrong slot declaration fails (WASM doesn't export required interfaces) — DEFERRED: negative test requires a deliberately mismatched WASM
- [x] Test: `init(config)` is called on extension load

**Files:** `scripts/smoke-test.sh`

## Acceptance Criteria

### Functional Requirements
- [ ] Each slot has a typed provider WIT interface
- [ ] Extensions export their slot's provider interface (structurally validated by wasmtime)
- [ ] All extensions import the same unified `host` interface for platform capabilities
- [ ] `extension.toml` is the source of truth for extension metadata
- [ ] Discovery reads TOML only — no WASM loading at discovery time
- [ ] `init(config)` replaces `register()` for runtime setup
- [ ] Host routes `host.*` calls to the correct active provider
- [ ] `call-tool()` still works for generic tool support
- [ ] All existing CLI commands work with the new system

### Non-Functional Requirements
- [ ] Discovery is faster (no WASM instantiation)
- [ ] Invalid extensions fail at instantiation, not discovery
- [ ] Type mismatches caught at WASM load time, not runtime

## Dependencies & Risks

- **New dependency**: `toml` crate (well-established, serde-compatible)
- **Risk**: Separate bindgen modules produce duplicate types for shared interfaces. Mitigated by `with` remapping if needed, or by accepting the duplication since shared types are small.
- **Risk**: All extensions must be rebuilt simultaneously. Acceptable for greenfield.
- **Risk**: Host routing adds complexity to `extension_host.rs`. Mitigated by keeping routing logic in a separate module if the file grows too large.

## References & Research

### Internal References
- Brainstorm: [.agents/brainstorms/2026-03-21-slot-typed-contracts-brainstorm.md](.agents/brainstorms/2026-03-21-slot-typed-contracts-brainstorm.md)
- Current WIT: [wit/world.wit](wit/world.wit)
- Extension host: [src/extension_host.rs](src/extension_host.rs)
- Slot definitions: [src/slot.rs](src/slot.rs)
- Discovery: [src/discovery.rs](src/discovery.rs)

### External References
- WIT Specification: https://github.com/WebAssembly/component-model/blob/main/design/mvp/WIT.md
- wasmtime bindgen docs: https://docs.wasmtime.dev/api/wasmtime/component/macro.bindgen.html
- wit-bindgen docs: https://docs.rs/wit-bindgen/latest/wit_bindgen/macro.generate.html
- wasmtime multi-world discussion: https://github.com/bytecodealliance/wasmtime/issues/8050
