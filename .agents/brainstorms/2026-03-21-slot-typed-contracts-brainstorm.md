# Slot-Typed Extension Contracts

**Date:** 2026-03-21
**Status:** Complete

## What We're Building

A typed contract system where each slot (llm-provider, session-provider, compaction-provider) defines two WIT interfaces:

- **Provider interface** (export): what an extension filling that slot must implement
- **Consumer interface** (import): what any extension can call to use that capability

The slot's cardinality guarantee (at-least-1, exactly-1) makes the consumer side safe at runtime — the capability is always available.

## Why This Approach

The current `call-tool(name, args_json) -> Result<string, string>` interface is intentionally generic. As we wire the agent loop, the host needs to make typed calls like "complete this chat" or "compact these messages." Routing through stringly-typed JSON means the contract is implicit and unenforced.

WIT gives us structural type safety at the WASM boundary. By defining per-slot interfaces, the contract is explicit, versioned, and enforced by the component model itself.

## Key Decisions

### 1. Separate WIT worlds per slot type

Optional exports are not supported in the WASM Component Model. A component must implement all exports in its world. Therefore, each slot gets its own world:

- `world llm-extension` — exports `llm-provider`, can import `session`, `compaction`
- `world session-extension` — exports `session-provider`, can import `llm`, `compaction`
- `world compaction-extension` — exports `compaction-provider`, can import `llm`, `session`
- `world general-extension` — no slot-specific export, generic tools only

All worlds share common type definitions and all export the base `extension` interface (for `init` and generic tool support).

### 2. Sidecar manifest replaces register()

Each extension ships an `extension.toml` next to its `.wasm`:

```toml
[extension]
id = "llm-openai"
name = "LLM OpenAI"
slot = "llm-provider"
wasm = "llm_openai.wasm"
```

Benefits:
- Discovery reads TOML, never loads WASM — enables lazy loading
- Slot is known before instantiation, so the host picks the right world
- Structural validation is free: wasmtime rejects WASM that doesn't export the declared interfaces

### 3. init(config) replaces register()

`register()` is removed from WIT. Its role is split:
- **Identity/slot** → sidecar TOML (static metadata)
- **Runtime setup** → `init(config)` function called on first load (API keys, connection strings, etc.)
- **Contract validation** → wasmtime instantiation against the correct world (structural)

### 4. Host mediates cross-extension calls

Extensions never call each other directly. They call typed host imports (`llm.complete()`, `session.load()`, etc.), and the host routes to the appropriate provider. This keeps extensions isolated while giving them access to platform capabilities guaranteed by the slot system.

### 5. Slot interfaces (rough shape)

**llm-provider:**
- `complete(messages, opts) -> result<completion, string>`

**session-provider:**
- `load(id) -> result<list<message>, string>`
- `append(id, message) -> result<_, string>`
- `list() -> result<list<session-info>, string>`

**compaction-provider:**
- `compact(messages) -> result<list<message>, string>`

Shared types (message, completion, complete-opts, etc.) live in a common `types` interface imported by all worlds.

## Open Questions

- **Multiple LLM providers**: When multiple are enabled (at-least-1 cardinality), how does the host choose which one to route `llm.complete()` to? User preference? Extension priority? Per-call selection?
- **init(config) shape**: What does the config record look like? Extension-specific key-value pairs? A JSON blob? Environment variables?
- **Error contract**: Should slot interfaces define structured error types, or is `result<T, string>` sufficient for now?
- **Streaming**: LLM completion is inherently streaming. The component model doesn't natively support streams yet. Polling pattern? Callback via host import? Defer until needed?
