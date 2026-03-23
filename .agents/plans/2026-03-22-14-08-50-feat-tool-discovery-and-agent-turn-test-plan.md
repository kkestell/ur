---
title: "feat: Tool discovery, tools-in-completion, and deterministic agent turn test"
type: feat
date: 2026-03-22
---

# Tool discovery, tools-in-completion, and deterministic agent turn test

## Overview

Extensions can handle tools (`call-tool`) but can't declare them. The host has no way to ask "what tools do you offer?" and can't tell the LLM what's available. This plan adds `list-tools` to the extension interface, passes tool descriptors to LLM providers, and creates a deterministic stub LLM that enables a full agent turn test without any API key.

## Problem Statement / Motivation

Two gaps remain after the self-describing extensions and multi-part messages work:

1. **No tool discovery.** The LLM can't request tool calls because it never learns what tools exist. The Google extension sends requests without a `tools` field, so Gemini has no function declarations to work with. Tool calls only worked in the original tracer bullet because the stub LLM hardcoded a `greet` call.

2. **No deterministic agent turn test.** The smoke test's integration section requires `GOOGLE_API_KEY`. Without it, there's zero coverage of the full turn: LLM call -> tool dispatch -> second LLM call. A deterministic stub LLM would exercise the complete loop in CI without external dependencies.

## What Changed Since the Original Plan

The self-describing extensions and multi-part messages plan was fully implemented. This simplifies the tool discovery work:

- **Tool result formatting is free.** `MessagePart::ToolResult` carries `tool_name` and `tool_call_id` natively. The original plan's JSON-in-content hack and `message_to_gemini` fix are unnecessary — `message_to_gemini` already handles `ToolResult` parts correctly.
- **No extension.toml.** The `llm-test` stub extension is self-describing via `id()` and `name()`. No TOML sidecar needed.
- **Smoke test model IDs are correct.** The smoke test already uses `google/gemini-3-flash-preview` and `google/gemini-3-pro-preview`.
- **Completion type is already simplified.** Tool calls live in `message.parts` — no separate `tool_calls` field on `Completion`.

## Proposed Solution

Three phases of work, each building on the previous:

1. **WIT changes** — Add `list-tools` to the `extension` interface and `tools` parameter to both completion functions
2. **Host plumbing** — Collect tool descriptors from general extensions and pass them through to LLM providers; reorder the turn loop
3. **Extension updates + stub LLM + smoke test** — Test extension declares its greet tool, Google extension formats tools as Gemini `functionDeclarations`, new deterministic `llm-test` extension, smoke test agent turn

### Architecture

```
Turn loop (turn.rs)
  |
  +- Load session
  +- Add user message
  |
  +- Load general extensions, init, call list_tools()
  |   +- test-extension returns: [{ name: "greet", description: "...", parameters: "..." }]
  |
  +- Collect all tool descriptors into Vec<ToolDescriptor>
  |
  +- LLM complete_streaming(messages, model, settings, tools)
  |   |                                               ^^^^^ NEW
  |   +- Google extension builds request body with:
  |       "tools": [{ "functionDeclarations": [...] }]
  |
  +- Tool dispatch (unchanged - call_tool on general extensions)
  |
  +- Second LLM complete_streaming(messages, model, settings, tools)
  |
  +- Append to session
  +- Compact
```

## Technical Approach

### Phase 1: WIT changes

**Goal:** Add tool discovery and tool-aware completion to the WIT interface.

The `tool-descriptor` type already exists in `wit/world.wit:112-116` and is already imported by the `extension` interface (`use types.{config-entry, tool-descriptor}` at line 157).

#### Changes to `interface extension`

Add `list-tools` alongside the existing functions:

```wit
interface extension {
    use types.{config-entry, tool-descriptor};

    init: func(config: list<config-entry>) -> result<_, string>;
    call-tool: func(name: string, args-json: string) -> result<string, string>;
    id: func() -> string;
    name: func() -> string;
    list-tools: func() -> list<tool-descriptor>;   // <- NEW
}
```

#### Changes to `interface llm-provider`

Add `tool-descriptor` to the `use` clause and `tools` parameter to `complete`:

```wit
interface llm-provider {
    use types.{message, completion, model-descriptor, config-setting, tool-descriptor};

    provider-id: func() -> string;
    list-models: func() -> list<model-descriptor>;
    complete: func(messages: list<message>, model: string, settings: list<config-setting>,
                   tools: list<tool-descriptor>) -> result<completion, string>;
}
```

#### Changes to `interface llm-streaming-provider`

Add `tool-descriptor` to the `use` clause and `tools` parameter to `complete-streaming`:

```wit
interface llm-streaming-provider {
    use types.{message, config-setting, completion-chunk, tool-descriptor};

    resource completion-stream {
        next: func() -> option<completion-chunk>;
    }

    complete-streaming: func(messages: list<message>, model: string,
                             settings: list<config-setting>,
                             tools: list<tool-descriptor>)
        -> result<completion-stream, string>;
}
```

**Breaking change:** This modifies the signature of `complete` and `complete-streaming`. The Google extension and any other LLM providers must be updated.

#### Tasks

- [x] Add `list-tools: func() -> list<tool-descriptor>` to `extension` interface in `wit/world.wit`
- [x] Add `tool-descriptor` to `llm-provider` interface's `use` clause
- [x] Add `tools: list<tool-descriptor>` parameter to `llm-provider.complete`
- [x] Add `tool-descriptor` to `llm-streaming-provider` interface's `use` clause
- [x] Add `tools: list<tool-descriptor>` parameter to `llm-streaming-provider.complete-streaming`

### Phase 2: Host plumbing

**Goal:** The host collects tool descriptors from general extensions and passes them to LLM completion calls.

#### `src/extension_host.rs` — new `list_tools` method

Add a method on `ExtensionInstance` that calls `list-tools` on the guest. All four variants implement `extension`, so all can be called. LLM/session/compaction extensions return `vec![]` in practice.

```rust
pub fn list_tools(&mut self) -> wasmtime::Result<Vec<wit_types::ToolDescriptor>> {
    match self {
        Self::Llm { store, bindings } => {
            bindings.ur_extension_extension().call_list_tools(store)
        }
        Self::Session { store, bindings } => {
            bindings.ur_extension_extension().call_list_tools(store)
        }
        Self::Compaction { store, bindings } => {
            bindings.ur_extension_extension().call_list_tools(store)
        }
        Self::General { store, bindings } => {
            bindings.ur_extension_extension().call_list_tools(store)
        }
    }
}
```

#### `src/extension_host.rs` — update `complete` and `complete_streaming`

Add `tools: &[wit_types::ToolDescriptor]` parameter to both methods. Pass it through to the guest's `call_complete` / `call_complete_streaming`.

Current signatures:
```rust
// complete at line 401
pub fn complete(&mut self, messages: &[wit_types::Message], model: &str, settings: &[wit_types::ConfigSetting])

// complete_streaming at line 425
pub fn complete_streaming(&mut self, messages: &[wit_types::Message], model: &str, settings: &[wit_types::ConfigSetting], on_chunk: ...)
```

New signatures:
```rust
pub fn complete(&mut self, messages: &[wit_types::Message], model: &str, settings: &[wit_types::ConfigSetting], tools: &[wit_types::ToolDescriptor])

pub fn complete_streaming(&mut self, messages: &[wit_types::Message], model: &str, settings: &[wit_types::ConfigSetting], tools: &[wit_types::ToolDescriptor], on_chunk: ...)
```

#### `src/turn.rs` — reorder turn loop

The turn loop currently loads general extensions lazily in step 4 (tool dispatch, `load_general_extensions` at line 168). With tool discovery, generals must be loaded **before** the first LLM call so their tools can be collected.

New order:

```
1. Load session
2. Add user message
3. Load general extensions, init each, collect tools   <- MOVED EARLIER
4. First LLM streaming(messages, model, settings, tools)
5. Tool dispatch to generals (already loaded)
6. Second LLM streaming(messages, model, settings, tools)
7. Append to session
8. Compact
```

`stream_completion` gains a `tools` parameter and passes it through to `complete_streaming`.

#### Tasks

- [x] Add `list_tools()` method to `ExtensionInstance` (calls `call_list_tools` on all four variants)
- [x] Update `complete()` signature to include `tools: &[wit_types::ToolDescriptor]`, pass through to guest
- [x] Update `complete_streaming()` signature to include `tools: &[wit_types::ToolDescriptor]`, pass through to guest
- [x] Update `stream_completion()` in `turn.rs` to accept and forward `tools`
- [x] Move general extension loading before first LLM call in `turn.rs`
- [x] Init each general extension and call `list_tools()` to collect descriptors
- [x] Pass collected tools to `stream_completion()` -> `complete_streaming()`
- [x] Update all extensions that implement `extension` to add `list_tools()` (session-jsonl, compaction-llm return empty vec)

### Phase 3: Extensions, stub LLM, and smoke test

**Goal:** Extensions implement the new WIT functions, a deterministic LLM enables full agent turn testing without any API key.

#### `extensions/workspace/test-extension/src/lib.rs` — declare greet tool

Add `list_tools()` returning the greet tool descriptor:

```rust
fn list_tools() -> Vec<ToolDescriptor> {
    vec![ToolDescriptor {
        name: "greet".into(),
        description: "Greet someone by name".into(),
        parameters_json_schema: r#"{"type":"object","properties":{"name":{"type":"string","description":"Name to greet"}},"required":["name"]}"#.into(),
    }]
}
```

#### `extensions/system/llm-google/src/lib.rs` — accept tools, format as functionDeclarations

Update `complete()` and `complete_streaming()` to accept the new `tools: Vec<ToolDescriptor>` parameter. Pass tools to `build_request_body`.

In `build_request_body`, add a `tools` parameter and build the Gemini `tools` field:

```rust
fn build_request_body(
    messages: &[Message],
    settings: &[ConfigSetting],
    tools: &[ToolDescriptor],   // <- NEW
) -> String {
    // ... existing code ...

    // Add tool declarations if any tools provided.
    if !tools.is_empty() {
        let declarations: Vec<serde_json::Value> = tools.iter().map(|t| {
            let params: serde_json::Value = serde_json::from_str(&t.parameters_json_schema)
                .unwrap_or(serde_json::json!({"type": "object"}));
            serde_json::json!({
                "name": t.name,
                "description": t.description,
                "parameters": params
            })
        }).collect();
        body.insert("tools".into(), serde_json::json!([{
            "functionDeclarations": declarations
        }]));
    }

    // ... rest ...
}
```

Note: `message_to_gemini` already correctly handles `MessagePart::ToolResult` with `tool_name` — no changes needed there.

#### `extensions/system/session-jsonl/src/lib.rs` and `extensions/system/compaction-llm/src/lib.rs`

Both need `list_tools()` added, returning empty vec:

```rust
fn list_tools() -> Vec<ToolDescriptor> {
    vec![]
}
```

#### `extensions/workspace/llm-test/` — deterministic stub LLM

New workspace extension using the `llm-extension` world (no HTTP import needed — pure deterministic logic).

```
extensions/workspace/llm-test/
+-- Cargo.toml          (wit-bindgen, serde_json)
+-- src/lib.rs
```

No `extension.toml` — the extension is self-describing via `id()` and `name()`, with slot auto-detected from its `llm-provider` export.

**Behavior:**

- `id()` -> `"llm-test"`
- `name()` -> `"Test LLM"`
- `provider_id()` -> `"test"`
- `list_models()` -> `[{ id: "echo", name: "Echo", is_default: true, settings: [] }]`
- `list_tools()` -> `[]` (LLM doesn't offer tools, it consumes them)
- `complete(messages, model, settings, tools)`:
  - If `tools` is non-empty AND no message has a `tool-result` part -> return a tool call for the first tool with dummy args `{"name": "world"}`
  - If any message has a `tool-result` part -> return text: `"Tool result received: {content}"`
  - If no tools -> return text echoing the last user message
- `complete_streaming(messages, model, settings, tools)` -> same logic, delivered as a single chunk followed by `None`

This is explicit about tool discovery — it only calls tools that were declared via the `tools` parameter.

#### `scripts/smoke-test.sh` — add deterministic agent turn test

Add a new section **before** the existing real-API integration test:

```bash
# -- Deterministic agent turn test -------------------------------------------
echo ""
echo "=== Agent turn test ==="

# Build the test LLM
cargo build --manifest-path "$ROOT/extensions/workspace/llm-test/Cargo.toml" \
    --target wasm32-wasip2 --release 2>&1

# Install it
mkdir -p "$WORKSPACE/.ur/extensions/llm-test"
copy_wasm "$WORKSPACE/.ur/extensions/llm-test" \
    "$ROOT/extensions/workspace/llm-test/target/wasm32-wasip2/release/llm_test.wasm"

# Enable test-extension (tool provider) and llm-test (deterministic LLM)
run extensions enable test-extension
run extensions enable llm-test

# Set default role to the deterministic test LLM
run model set default test/echo

# Run a full agent turn
OUTPUT="$(UR_ROOT="$UR_ROOT" "$UR" -w "$WORKSPACE" run 2>&1)"
echo "$OUTPUT"

# Verify the full turn loop fired
for expected in \
    "[turn] loading session" \
    "[turn] session loaded" \
    "[turn] adding user message" \
    "[turn] calling LLM streaming" \
    "[turn] LLM returned tool call" \
    "[turn] dispatching tool" \
    "[turn] tool result" \
    "[turn] LLM returned message" \
    "[turn] appending assistant message" \
    "[turn] compacting" \
    "[turn] done"
do
    if ! echo "$OUTPUT" | grep -qF "$expected"; then
        echo "FAIL: missing expected output: $expected"
        exit 1
    fi
done
echo "Agent turn test passed."
```

This exercises: session load -> user message -> tool discovery -> LLM with tools -> tool call -> tool dispatch -> tool result -> second LLM -> session append -> compact — all without a real API key.

Also update the build section to include `llm-test`, and add it to the extension setup.

#### Tasks

- [x] Test extension: implement `list_tools()` returning greet tool descriptor
- [x] Google extension: add `tools` parameter to `complete()` and `complete_streaming()`
- [x] Google extension: update `build_request_body()` to accept and format `tools` as Gemini `functionDeclarations`
- [x] Google extension: implement `list_tools()` returning empty vec
- [x] Session-jsonl extension: implement `list_tools()` returning empty vec
- [x] Compaction-llm extension: implement `list_tools()` returning empty vec
- [x] Create `extensions/workspace/llm-test/Cargo.toml` with wit-bindgen and serde_json deps
- [x] Implement `llm-test/src/lib.rs` with deterministic tool-call-then-text behavior
- [x] Add llm-test to smoke test build step
- [x] Add deterministic agent turn test section to smoke test (before the real API test)
- [x] Verify smoke test passes without `GOOGLE_API_KEY`

## Engineering Quality

| Principle | Application |
|-----------|-------------|
| **SRP** | `list_tools()` is a pure query — separate from `call_tool()` dispatch. Tool collection logic stays in `turn.rs`, formatting stays in extensions. |
| **OCP / DIP** | LLM providers receive `list<tool-descriptor>` — they format for their own API without knowing which extensions provided the tools. Adding a new LLM provider or tool-providing extension requires no changes to the host. |
| **YAGNI** | No tool registry, no tool namespacing, no tool versioning. Extensions declare, host collects, LLM receives — the simplest possible pipeline. |
| **Testability** | The `llm-test` extension makes the full turn loop testable without external dependencies. Tool discovery is exercised in CI on every build. |
| **Value Objects** | `tool-descriptor` and `tool-result` are proper WIT records — tool metadata flows through the system as structured data, never as convention-encoded strings. |

## Acceptance Criteria

- [x] `list-tools` exists on the WIT `extension` interface
- [x] `complete` and `complete-streaming` accept a `tools` parameter
- [x] Test extension's `list_tools()` returns the greet tool descriptor
- [x] Host collects tools from general extensions before the first LLM call
- [x] Google extension includes `functionDeclarations` in Gemini API request when tools are provided
- [x] Google extension omits `tools` field when no tools are provided
- [x] `llm-test` extension returns tool calls when tools are declared, text otherwise
- [x] Smoke test exercises full turn loop (tool call -> dispatch -> second LLM) without API key
- [x] All existing smoke tests still pass
- [x] `cargo build` succeeds for host and all extensions

## References

### Internal

- [wit/world.wit](wit/world.wit) — WIT interface definitions, `tool-descriptor` at line 112, `extension` at line 156
- [src/extension_host.rs](src/extension_host.rs) — `ExtensionInstance` methods, `complete_streaming` at line 425
- [src/turn.rs](src/turn.rs) — turn loop orchestrator, tool dispatch at line 166, general extension loading at line 168
- [extensions/system/llm-google/src/lib.rs](extensions/system/llm-google/src/lib.rs) — `build_request_body` at line 274, `message_to_gemini` at line 342
- [extensions/workspace/test-extension/src/lib.rs](extensions/workspace/test-extension/src/lib.rs) — greet tool handler at line 17
- [scripts/smoke-test.sh](scripts/smoke-test.sh) — smoke test suite

### Prior Plans

- `.agents/plans/2026-03-22-feat-self-describing-extensions-and-multi-part-messages-plan.md` — implemented; eliminated extension.toml, added multi-part messages, fixed tool result formatting
- `.agents/plans/2026-03-22-feat-agent-turn-tracer-bullet-plan.md` — original turn loop design
- `.agents/brainstorms/2026-03-22-llm-google-streaming-brainstorm.md` — tool use decisions
