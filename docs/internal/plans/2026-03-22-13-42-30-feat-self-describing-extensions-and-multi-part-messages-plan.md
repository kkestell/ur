---
title: "feat: Self-describing extensions and multi-part messages"
type: feat
date: 2026-03-22
---

# Self-describing extensions and multi-part messages

## Overview

Eliminate `extension.toml` sidecar files. Extensions become fully self-describing WASM components: the host compiles them, inspects their exports to detect slot, and calls WIT functions to learn identity. Simultaneously redesign `Message` from `{ role, content }` to a multi-part variant type that properly models text, tool calls, and tool results — eliminating the JSON-in-content hack and enabling correct Gemini `functionResponse` formatting.

## Problem Statement / Motivation

**extension.toml is a liability.** It duplicates what the compiled WASM already knows (id, name, slot). Any new metadata (tools, capabilities) either goes in the TOML (risking drift with the code) or requires loading WASM anyway (defeating the TOML's purpose). The toml crate dependency exists solely for this.

**Message is too bare.** `Message { role: string, content: string }` can't carry tool call metadata. Gemini requires `functionResponse.name` and `functionResponse.id` to match the original call. The tool discovery plan (`feat-tool-discovery-and-agent-turn-test-plan.md`) works around this by encoding metadata as JSON in the content string — a hack that every LLM provider must parse. A proper variant type models what messages actually contain and matches how Gemini and Anthropic both represent content (multi-part blocks).

**Slot detection requires TOML today.** The `slot` field in extension.toml is the only way the host knows which world to instantiate against. But wasmtime's `Component::component_type()` exposes export information pre-instantiation — the host can inspect which interfaces a component exports and determine the slot from that.

## Proposed Solution

Four phases, each building on the previous:

1. **WIT changes** — Add `id()`, `name()` identity functions and `message-part` variant type
2. **Slot detection** — Inspect component exports via wasmtime's type system (no instantiation)
3. **Discovery rework** — Scan for `.wasm` files, compile + inspect + instantiate for metadata, delete extension.toml
4. **Multi-part message adoption** — Update all extensions and the turn loop to use the new message type

## Technical Considerations

- **Startup latency:** Loading all WASM at startup adds time to every command. wasmtime has compilation caching that may mitigate this. If it becomes a problem, compiled component metadata could be cached — but YAGNI for now.
- **Export naming convention:** WIT interfaces in package `ur:extension@0.2.0` export as `ur:extension/llm-provider@0.2.0`. The exact string must be verified at implementation time by printing actual export names from a compiled component.
- **Session serialization breaking change:** The multi-part message format changes the shape of persisted sessions. Acceptable — greenfield, no backwards compatibility.
- **Completion record simplification:** With tool calls in message parts, the separate `tool_calls: list<tool-call>` field on `completion` becomes redundant. Remove it — `completion` becomes `{ message, usage }`.

### Engineering Quality

| Principle | Application |
|-----------|-------------|
| **SRP** | Identity is the extension's responsibility (id/name functions), slot detection is the host's responsibility (type inspection). Discovery does one thing: find and catalog extensions. |
| **OCP / DIP** | Adding a new slot type requires only a new WIT interface export — no changes to the discovery or type inspection code, just a new entry in the slot-to-export mapping. |
| **YAGNI** | No metadata caching, no lazy loading, no extension packaging format. Compile at startup, inspect, done. |
| **Value Objects** | `message-part` is a proper variant (sum type) — text, tool-call, and tool-result are distinct domain concepts, not strings with conventions. |

## Technical Approach

### Phase 1: WIT changes

**Goal:** Add self-description functions and multi-part message type to the WIT interface.

#### Identity functions on `extension` interface

Add `id` and `name` alongside existing `init` and `call-tool`:

```wit
interface extension {
    use types.{config-entry, tool-descriptor};

    init: func(config: list<config-entry>) -> result<_, string>;
    call-tool: func(name: string, args-json: string) -> result<string, string>;
    id: func() -> string;      // ← NEW
    name: func() -> string;    // ← NEW
}
```

Every extension already implements `extension`. Adding two pure functions is minimal surface area.

#### Multi-part message type

Replace the flat `message` record with a variant-based structure:

```wit
/// A single part of a message — text, tool call, or tool result.
variant message-part {
    text(string),
    tool-call(tool-call),
    tool-result(tool-result),
}

/// The result of a tool invocation, linked back to the original call.
record tool-result {
    tool-call-id: string,
    tool-name: string,
    content: string,
}

/// A single message in a conversation.
record message {
    role: string,
    parts: list<message-part>,
}
```

The existing `tool-call` record (`{ id, name, arguments_json }`) is reused as-is inside the `tool-call` variant.

#### Simplify `completion`

With tool calls now in message parts, the separate `tool-calls` field is redundant:

```wit
/// Before:
record completion {
    message: message,
    tool-calls: list<tool-call>,   // ← REMOVE
    usage: option<usage>,
}

/// After:
record completion {
    message: message,
    usage: option<usage>,
}
```

Similarly, `completion-chunk` drops `delta-tool-calls` — tool call deltas arrive as `message-part` variants in `delta-parts`:

```wit
record completion-chunk {
    delta-parts: list<message-part>,     // ← REPLACES delta-text + delta-tool-calls
    finish-reason: option<string>,
    usage: option<usage>,
}
```

#### Tasks

- [x] Add `id: func() -> string` to `extension` interface in `wit/world.wit`
- [x] Add `name: func() -> string` to `extension` interface in `wit/world.wit`
- [x] Add `tool-result` record to `types` interface
- [x] Add `message-part` variant to `types` interface
- [x] Change `message` from `{ role, content }` to `{ role, parts }`
- [x] Remove `tool-calls` from `completion` record
- [x] Replace `delta-text` and `delta-tool-calls` on `completion-chunk` with `delta-parts: list<message-part>`
- [x] Verify all four worlds still parse cleanly (`cargo component check` or equivalent)

### Phase 2: Slot detection via component type inspection

**Goal:** Determine an extension's slot from its compiled component exports, without instantiation.

#### wasmtime API

The `Component::component_type()` method returns a `types::Component` which supports:

```rust
// Iterate all exports
let ct = component.component_type();
for (name, item) in ct.exports(&engine) {
    // name: "ur:extension/llm-provider@0.2.0"
    // item: ComponentItem::ComponentInstance(...)
}

// Or check for specific exports
if ct.get_export(&engine, "ur:extension/llm-provider@0.2.0").is_some() {
    // This component targets the llm-provider slot
}
```

#### Slot-to-export mapping

Add a function in `src/slot.rs` that maps export names to slots:

```rust
/// Detect slot by inspecting component exports. Returns None for general extensions.
pub fn detect_slot(engine: &Engine, component: &Component) -> Option<&'static str> {
    let ct = component.component_type();
    // Order matters: check most specific first
    if ct.get_export(engine, "ur:extension/llm-provider@0.2.0").is_some() {
        Some("llm-provider")
    } else if ct.get_export(engine, "ur:extension/session-provider@0.2.0").is_some() {
        Some("session-provider")
    } else if ct.get_export(engine, "ur:extension/compaction-provider@0.2.0").is_some() {
        Some("compaction-provider")
    } else {
        None // general extension
    }
}
```

**Risk:** The exact export name format needs verification. At implementation time, print all exports from a compiled test component to confirm the naming convention. The names may or may not include the version suffix.

#### Update `ExtensionInstance::load`

Currently `load()` takes an explicit `slot: Option<&str>` parameter to choose which bindgen world to instantiate against. Replace this with automatic detection:

```rust
pub fn load(engine: &Engine, path: &Path) -> Result<Self> {
    let component = Component::from_file(engine, path)?;
    let slot = slot::detect_slot(engine, &component);
    // Use slot to choose correct bindings...
}
```

#### Tasks

- [x] Verify export name format by printing exports from a compiled component
- [x] Add `detect_slot(engine, component) -> Option<&str>` to `src/slot.rs`
- [x] Update `ExtensionInstance::load` to auto-detect slot from component type (remove explicit slot parameter)
- [x] Update all call sites of `load()` to drop the slot argument

### Phase 3: Discovery rework (eliminate extension.toml)

**Goal:** Extensions are discovered by scanning for `.wasm` files. Identity comes from calling `id()` and `name()` on the loaded component. extension.toml files are deleted.

#### New discovery flow

```
extensions/{system,user}/           ← source tiers
  └─ <ext-dir>/
      └─ target/wasm32-wasip2/release/<crate_name>.wasm

.ur/extensions/                     ← workspace tier (installed extensions)
  └─ <ext-dir>/
      └─ <name>.wasm               ← or target/wasm32-wasip2/release/
```

For each extension subdirectory:

1. **Find .wasm file.** Scan for `*.wasm` files. For source extensions: look in `target/wasm32-wasip2/release/`. For workspace: look in the directory root and `target/` subtree. Take the first `.wasm` found (error if none, warn if multiple).

2. **Compile component.** `Component::from_file(engine, &wasm_path)` — this is pre-instantiation.

3. **Detect slot.** Use `detect_slot(engine, &component)` from Phase 2.

4. **Instantiate and query identity.** Create a minimal `ExtensionInstance`, call `id()` and `name()`.

5. **Build `DiscoveredExtension`.** Same struct as today but populated from runtime data instead of TOML.

6. **Duplicate ID check.** Same as current behavior — error if two extensions return the same `id()`.

#### Changes to `src/discovery.rs`

The `discover()` function currently:
- Walks directories for `extension.toml`
- Parses TOML for metadata
- Returns `Vec<DiscoveredExtension>`

New version:
- Walks directories for `.wasm` files
- Compiles each component
- Inspects type for slot
- Instantiates and calls `id()` / `name()`
- Returns `Vec<DiscoveredExtension>`

The function signature gains an `engine: &Engine` parameter since compilation requires it.

```rust
pub fn discover(engine: &Engine, ur_root: &Path, workspace: &Path) -> Result<Vec<DiscoveredExtension>>
```

#### Remove `ExtensionToml` struct

Delete the `ExtensionToml` struct and all TOML parsing from `discovery.rs`. The `toml` dependency may still be needed for `config.toml` — check before removing.

#### Delete extension.toml files

Remove all sidecar files:
- `extensions/system/session-jsonl/extension.toml`
- `extensions/system/compaction-llm/extension.toml`
- `extensions/system/llm-google/extension.toml`
- `extensions/workspace/test-extension/extension.toml`

#### Update manifest merge

`manifest::load_or_create` currently calls `discovery::discover()` without an engine. Update the call site in `src/main.rs` to pass the engine through. The merge logic itself doesn't change — it still matches by extension ID and preserves enabled state.

#### Update smoke test

The smoke test's `write_toml` helper creates extension.toml files. Replace with a mechanism that either:
- Builds extensions and lets discovery find the `.wasm` artifacts, or
- Copies `.wasm` files to a known location under `.ur/extensions/`

The test should verify that extensions are discovered from their built artifacts without any TOML.

#### Tasks

- [x] Update `discover()` signature to accept `&Engine`
- [x] Replace TOML scanning with `.wasm` file scanning in `discover()`
- [x] Compile each discovered `.wasm` into a `Component`
- [x] Call `detect_slot()` for each component
- [x] Instantiate each component and call `id()` / `name()` for identity
- [x] Remove `ExtensionToml` struct and TOML parsing from `discovery.rs`
- [x] Delete all `extension.toml` files from extension directories
- [x] Update `main.rs` to pass `&Engine` through to discovery
- [x] Update smoke test to work without `write_toml` / extension.toml
- [x] Check if `toml` crate is still needed (config.toml uses it) — keep or remove accordingly

### Phase 4: Multi-part message adoption

**Goal:** All extensions and the turn loop use the new multi-part message type.

#### `extensions/system/llm-google/src/lib.rs`

The Google extension is the most affected — it must translate between ur's `message-part` variants and Gemini's content format.

**`build_request_body` changes:**

Convert `message.parts` to Gemini `parts` array:

```rust
fn message_parts_to_gemini(parts: &[MessagePart]) -> Vec<serde_json::Value> {
    parts.iter().map(|part| match part {
        MessagePart::Text(s) => json!({ "text": s }),
        MessagePart::ToolCall(tc) => json!({
            "functionCall": {
                "name": tc.name,
                "args": serde_json::from_str(&tc.arguments_json).unwrap_or(json!({}))
            }
        }),
        MessagePart::ToolResult(tr) => json!({
            "functionResponse": {
                "name": tr.tool_name,
                "response": { "result": tr.content }
            }
        }),
    }).collect()
}
```

**Tool result formatting fixed:** The current `message_to_gemini` for `role: "tool"` hardcodes `"name": "tool"`. With `MessagePart::ToolResult`, the `tool_name` and `tool_call_id` are first-class fields — no JSON parsing needed.

**Streaming response parsing:** `parse_sse_chunk` currently extracts `delta_text` and `delta_tool_calls` separately. Update to produce `delta_parts: Vec<MessagePart>` instead:
- Text deltas → `MessagePart::Text(delta)`
- Function calls → `MessagePart::ToolCall(tc)`

**`id()` and `name()` implementation:**

```rust
fn id() -> String { "llm-google".into() }
fn name() -> String { "Google Gemini".into() }
```

#### `extensions/workspace/test-extension/src/lib.rs`

```rust
fn id() -> String { "test-extension".into() }
fn name() -> String { "Test Extension".into() }
```

#### `extensions/system/session-jsonl/src/lib.rs`

Update to handle `Message { role, parts }` instead of `Message { role, content }`. The serialization format changes — the stub currently does nothing, so the change is trivial.

```rust
fn id() -> String { "session-jsonl".into() }
fn name() -> String { "Session JSONL".into() }
```

#### `extensions/system/compaction-llm/src/lib.rs`

Same pattern — update Message handling and add identity functions.

```rust
fn id() -> String { "compaction-llm".into() }
fn name() -> String { "Compaction LLM".into() }
```

#### `src/turn.rs` — turn loop updates

**Creating messages:** Replace `Message { role, content }` with part-based construction:

```rust
// User message
let user_msg = Message {
    role: "user".into(),
    parts: vec![MessagePart::Text("Hello, please greet the world".into())],
};

// Tool result message
let result_msg = Message {
    role: "tool".into(),
    parts: vec![MessagePart::ToolResult(ToolResult {
        tool_call_id: tc.id.clone(),
        tool_name: tc.name.clone(),
        content: result,
    })],
};
```

**Extracting tool calls:** Instead of reading `completion.tool_calls`, extract from message parts:

```rust
let tool_calls: Vec<&ToolCall> = completion.message.parts.iter()
    .filter_map(|p| match p {
        MessagePart::ToolCall(tc) => Some(tc),
        _ => None,
    })
    .collect();
```

**Streaming accumulation:** `stream_completion` currently accumulates `delta_text` into a string and `delta_tool_calls` into a vec. Update to accumulate `delta_parts` into the final message's `parts` list. Text deltas are concatenated into the current text part; tool call deltas append new parts.

#### `src/extension_host.rs` — bindgen type updates

The `wit_types` re-export includes `Message`, `Completion`, `CompletionChunk`. These change shape automatically when the WIT changes and the bindings are regenerated. Update any host-side code that constructs or destructures these types.

#### Tasks

- [x] Google extension: implement `id()` and `name()`
- [x] Google extension: update `build_request_body` to convert `message.parts` to Gemini format
- [x] Google extension: update `message_to_gemini` to handle `MessagePart` variants (eliminate role-based switching for tool results)
- [x] Google extension: update streaming response parsing to produce `delta_parts`
- [x] Google extension: update `complete()` and `complete_streaming()` for new `Completion` shape (no `tool_calls` field)
- [x] Test extension: implement `id()` and `name()`
- [x] Session-jsonl extension: implement `id()` and `name()`, update Message handling
- [x] Compaction-llm extension: implement `id()` and `name()`, update Message handling
- [x] Turn loop: construct messages with `parts` instead of `content`
- [x] Turn loop: extract tool calls from `message.parts` instead of `completion.tool_calls`
- [x] Turn loop: update `stream_completion` to accumulate `delta_parts`
- [x] Turn loop: construct tool result messages with `MessagePart::ToolResult`
- [x] Extension host: update any host-side code constructing/destructuring `Message`, `Completion`, or `CompletionChunk`
- [x] Verify all extensions compile against updated WIT

## Acceptance Criteria

- [x] `extension` interface exports `id()` and `name()` functions
- [x] All four extensions implement `id()` and `name()` returning correct values
- [x] `detect_slot()` correctly identifies llm-provider, session-provider, compaction-provider, and general extensions from compiled components
- [x] `ExtensionInstance::load` auto-detects slot without an explicit parameter
- [x] Discovery scans for `.wasm` files — no `extension.toml` in the codebase
- [x] Discovery calls `id()` and `name()` on each extension to populate manifest metadata
- [x] Duplicate extension IDs are detected and produce an error
- [x] `Message` has `parts: list<message-part>` with text, tool-call, and tool-result variants
- [x] `Completion` has no separate `tool_calls` field — tool calls live in message parts
- [x] `CompletionChunk` uses `delta_parts` instead of `delta_text` + `delta_tool_calls`
- [x] Google extension converts `MessagePart::ToolResult` to correct Gemini `functionResponse` format (with name and id)
- [x] Google extension converts `MessagePart::ToolCall` to Gemini `functionCall` format
- [x] Turn loop creates tool result messages with proper `ToolResult` parts
- [x] Smoke test passes without any extension.toml files
- [x] `cargo build` succeeds for host and all extensions
- [x] All existing unit tests pass

## Dependencies & Risks

**Dependency on wasmtime export naming.** The exact string format of export names (e.g., `ur:extension/llm-provider@0.2.0` vs `ur:extension/llm-provider`) must be verified empirically. Mitigation: print all exports from a test component as the first implementation step.

**Phase ordering.** Phases 1-2 can land independently. Phase 3 depends on both. Phase 4 (multi-part messages) could theoretically land independently of Phase 3, but doing them together avoids double-touching the same code.

**Interaction with tool discovery plan.** The existing `feat-tool-discovery-and-agent-turn-test-plan.md` adds `list-tools` and a `tools` parameter to completion functions. That plan should be implemented AFTER this one, since:
- Multi-part messages eliminate the JSON-in-content hack for tool results
- Self-describing extensions remove the need for extension.toml in the llm-test stub
- The tool discovery plan's Phase 3 (tool result formatting) becomes trivial with `MessagePart::ToolResult`

If tool discovery lands first, the multi-part message phase here would need to update the `tools` parameter and `list-tools` function as well.

## References

### Internal

- [wit/world.wit](wit/world.wit) — WIT interface definitions, current Message at line 6
- [src/extension_host.rs](src/extension_host.rs) — ExtensionInstance::load, slot parameter
- [src/discovery.rs](src/discovery.rs) — ExtensionToml parsing, discover() function
- [src/manifest.rs](src/manifest.rs) — manifest merge, ManifestEntry struct
- [src/slot.rs](src/slot.rs) — slot definitions and validation
- [src/turn.rs](src/turn.rs) — turn loop, message construction, tool dispatch
- [extensions/system/llm-google/src/lib.rs](extensions/system/llm-google/src/lib.rs) — message_to_gemini, build_request_body
- [scripts/smoke-test.sh](scripts/smoke-test.sh) — write_toml helper, integration tests

### Prior Work

- `.agents/brainstorms/2026-03-22-self-describing-extensions-brainstorm.md` — key decisions driving this plan
- `.agents/plans/2026-03-22-feat-tool-discovery-and-agent-turn-test-plan.md` — complementary plan (should land after this)
- `.agents/plans/2026-03-21-feat-slot-typed-extension-contracts-plan.md` — established the current WIT world structure

### External

- [wasmtime Component API](https://docs.wasmtime.dev/api/wasmtime/component/struct.Component.html) — `component_type()`, `get_export()`
- [wasmtime types::Component](https://docs.wasmtime.dev/api/wasmtime/component/types/struct.Component.html) — `exports()`, `get_export()`
- [wasmtime ComponentItem](https://docs.wasmtime.dev/api/wasmtime/component/types/enum.ComponentItem.html) — export type variants
