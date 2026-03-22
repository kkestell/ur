# Brainstorm: Self-Describing Extensions and Tool Discovery

**Date:** 2026-03-22
**Status:** Complete

## What We're Building

Eliminate `extension.toml` sidecar files. Extensions are fully self-describing WASM components. The host loads them, inspects their exports, and calls WIT functions to learn identity, capabilities, and tools. This naturally solves tool discovery (no duplication, no drift) and fixes the bare `Message` type that can't carry tool result metadata.

**Scope:** Remove extension.toml, add self-description to WIT, add tool discovery, redesign Message as multi-part, add deterministic agent turn test.

## Why This Approach

### Single source of truth

`extension.toml` duplicated information that the code already knew (id, name, slot). Any new metadata (tools, capabilities) would either go in the toml (risking drift) or require loading WASM anyway (defeating the toml's purpose). Eliminating the toml means the compiled WASM is the only authority.

### No lazy-loading tension

The earlier plan proposed `list-tools` in WIT but then struggled with when to call it — loading extensions upfront "defeats" lazy loading, static toml declarations drift. With no toml, the host loads all discovered extensions at startup. Every `ur` command knows what's available. The manifest still tracks enabled/disabled state per workspace; it just gets its metadata from loaded extensions instead of toml files.

### Multi-part messages

The `Message` type was `{ role: string, content: string }` — too bare for tool results. Gemini needs `functionResponse.name` and `functionResponse.id` to match the original call. Stuffing metadata into the content string as JSON is a hack. A proper variant type models what messages actually contain: text, tool calls, or tool results. This matches how Gemini and Anthropic both represent content (multi-part blocks).

## Key Decisions

1. **No extension.toml** — the WASM component is the metadata
2. **Discovery convention** — one subdirectory = one extension. Host scans `extensions/{system,user,workspace}/` for subdirectories containing `.wasm` files
3. **Slot detection** — inspect component exports via wasmtime's type system. Exports `llm-provider` → LLM slot. No function call needed, no instantiation.
4. **Identity via WIT** — add `id()` and `name()` functions to the `extension` interface. The extension declares its own identity.
5. **Tool discovery via WIT** — add `list-tools()` to the `extension` interface. Host calls it on loaded general extensions, passes results to LLM providers.
6. **Tools parameter** — `complete` and `complete-streaming` gain a `tools: list<tool-descriptor>` parameter. LLM providers format for their API (Gemini → `functionDeclarations`).
7. **Multi-part message type** — `Message` gets `parts: list<message-part>` with a variant for text, tool-calls, and tool-result. Completion record drops its separate `tool-calls` field — tool calls live in the message parts.

## Open Questions

- **Startup cost:** Loading all WASM at startup adds latency to every command. Is this acceptable? wasmtime has compilation caching that may mitigate this. If it becomes a problem, compiled component metadata could be cached — but YAGNI for now.
- **Component type inspection API:** Need to verify wasmtime's `Component::component_type()` provides enough export info to determine the slot without instantiation.
- **Session serialization:** The session-jsonl extension currently persists `Vec<Message>` with `{ role, content }`. The multi-part message format changes the serialization shape. This is a breaking change to stored sessions (acceptable — greenfield).
- **Completion simplification:** With tool calls in message parts, does the `completion` record become just `{ message, usage }`? Or is there value in keeping tool calls separate on the completion for host-side convenience?
