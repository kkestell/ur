---
title: "feat: Wire up agent turn tracer bullet"
type: feat
date: 2026-03-22
---

# Wire up agent turn tracer bullet

## Overview

Run through a full agent turn with all four provider types firing in the right order, using realistic but dummy data. A new `ur run` CLI subcommand orchestrates the turn at the host level, calling each provider extension directly — no cross-extension routing yet.

## Problem Statement / Motivation

The host infrastructure (extension loading, manifest, model resolution, config) is complete, but no code exercises it end-to-end. The `Host` trait callbacks all return "not yet routed." There is no agent turn/loop structure. This tracer bullet proves the full data path works before layering on real logic.

## Proposed Solution

Host-driven orchestration in a new `turn.rs` module. The host drives the turn by calling provider extension exports in sequence, avoiding wasmtime re-entrancy entirely. Cross-extension routing (host callbacks) is deferred.

### Expected output

```
[turn] loading session "demo"...
[turn] session loaded: 0 messages (fresh session)
[turn] adding user message: "Hello, please greet the world"
[turn] resolving role "default" → anthropic/claude-sonnet-4-6
[turn] calling LLM complete (1 message)...
[turn] LLM returned tool call: greet({"name":"world"})
[turn] dispatching tool "greet" to test-extension...
[turn] tool result: "Hello, world!"
[turn] calling LLM complete (3 messages, includes tool result)...
[turn] LLM returned message: "I greeted the world for you!"
[turn] appending assistant message to session "demo"
[turn] compacting 4 messages...
[turn] compaction result: 4 messages (unchanged)
[turn] done
```

### Turn flow

```
load session → add user msg → LLM complete → tool dispatch → LLM complete → append session → compact
     (1)           (2)            (3)             (4)             (5)            (6)          (7)
```

## Technical Considerations

### Architecture

- **Host-driven, not extension-driven.** The turn module calls provider exports directly (`llm.complete()`, `session.load_session()`, etc.). Extensions do not call `host.complete()` during this flow, so wasmtime re-entrancy is not a concern.
- **Provider lookup by slot.** Load the right extension instance for each slot from the manifest. For LLM, resolve the role to a specific provider/model first, then load only that provider's WASM.
- **Tool dispatch by iteration.** Load all enabled general extensions, try `call_tool` on each until one succeeds. No tool registry needed yet.

### WIT type extension

The current `completion` record has no way to express tool calls. Add minimal types:

```wit
record tool-call {
    id: string,
    name: string,
    arguments-json: string,
}

record completion {
    message: message,
    tool-calls: list<tool-call>,   // ← new field
    usage: option<usage>,
}
```

This is a breaking change to `completion`. All five extension stubs need updating to compile with the new shape (most just set `tool_calls: vec![]`).

### What is NOT in scope

- Cross-extension routing (host callbacks remain "not yet routed")
- Tool discovery protocol (no `list-tools` WIT method yet)
- Streaming
- Real LLM API calls
- Real session persistence
- Interactive input / multi-turn loop
- Threshold-based compaction triggers

### Engineering Quality

| Principle | Application |
|-----------|-------------|
| **SRP** | `turn.rs` owns orchestration only — delegates to existing modules for model resolution, config, manifest |
| **OCP / DIP** | Turn calls `ExtensionInstance` methods (the existing abstraction), not world-specific bindings directly |
| **YAGNI** | No tool registry, no streaming, no multi-turn — just the single tracer bullet |

## Acceptance Criteria

- [x] `ur run` executes a single hardcoded turn and prints debug output showing every step
- [x] Session provider called: `load_session("demo")` and `append_session("demo", msg)`
- [x] LLM provider called twice: first returns a tool call, second returns text
- [x] Tool dispatched to test-extension's `call_tool("greet", ...)`
- [x] Compaction provider called with full message history
- [x] All debug prints fire in the correct order shown above
- [x] `cargo build` succeeds with updated WIT types
- [x] All existing tests pass (60 unit tests + smoke test)
- [x] Smoke test updated to exercise `ur run`

## MVP

### Phase 1: WIT type changes

Add `tool-call` record and update `completion` in WIT. Update all extension stubs to compile.

#### `wit/world.wit` — add to `interface types`

```wit
/// A tool call requested by the LLM.
record tool-call {
    id: string,
    name: string,
    arguments-json: string,
}
```

Update `completion`:

```wit
record completion {
    message: message,
    tool-calls: list<tool-call>,
    usage: option<usage>,
}
```

#### `extensions/user/llm-anthropic/src/lib.rs` — update complete()

Return `tool_calls: vec![]` for now (behavior change in phase 2).

#### `extensions/system/llm-openai/src/lib.rs` — update complete()

Return `tool_calls: vec![]`.

### Phase 2: Extension stub behavior

#### `extensions/workspace/test-extension/src/lib.rs` — add greet tool

```rust
fn call_tool(name: String, args_json: String) -> Result<String, String> {
    match name.as_str() {
        "greet" => {
            // Parse name from args_json, return greeting
            Ok(format!("Hello, world! (args: {args_json})"))
        }
        other => Err(format!("unknown tool: {other}")),
    }
}
```

#### `extensions/user/llm-anthropic/src/lib.rs` — tool call on first completion

Use a message-count heuristic: if no message with `role == "tool"` exists in the history, return a tool call. Otherwise return text.

```rust
fn complete(messages: Vec<Message>, _model: String, _settings: Vec<ConfigSetting>) -> Result<Completion, String> {
    let has_tool_result = messages.iter().any(|m| m.role == "tool");

    if has_tool_result {
        // Second call: return final text
        Ok(Completion {
            message: Message { role: "assistant".into(), content: "I greeted the world for you!".into() },
            tool_calls: vec![],
            usage: Some(Usage { input_tokens: 0, output_tokens: 0 }),
        })
    } else {
        // First call: return tool call
        Ok(Completion {
            message: Message { role: "assistant".into(), content: String::new() },
            tool_calls: vec![ToolCall {
                id: "tc_001".into(),
                name: "greet".into(),
                arguments_json: r#"{"name":"world"}"#.into(),
            }],
            usage: Some(Usage { input_tokens: 0, output_tokens: 0 }),
        })
    }
}
```

### Phase 3: Turn orchestrator

#### `src/turn.rs` — new module

Single public function `run()` that orchestrates the full turn:

1. Load manifest and config (reuse existing functions)
2. Find and load each provider by slot from the manifest
3. Resolve "default" role to provider/model
4. Load session → print
5. Add hardcoded user message → print
6. First LLM complete → print
7. Check tool_calls, dispatch to general extensions → print
8. Second LLM complete with tool result → print
9. Append to session → print
10. Compact → print

Provider loading helpers (private to this module):

```rust
/// Finds the first enabled entry for a slot and loads it.
fn load_slot(engine: &Engine, manifest: &WorkspaceManifest, slot: &str) -> Result<ExtensionInstance>

/// Loads the LLM provider matching a specific provider ID.
fn load_llm_provider(engine: &Engine, manifest: &WorkspaceManifest, provider_id: &str) -> Result<ExtensionInstance>

/// Loads all enabled general extensions (for tool dispatch).
fn load_general_extensions(engine: &Engine, manifest: &WorkspaceManifest) -> Result<Vec<ExtensionInstance>>
```

### Phase 4: CLI wiring

#### `src/cli.rs` — add Run command

```rust
pub enum Command {
    Extensions { ... },
    Model { ... },
    /// Run a single agent turn (tracer bullet).
    Run,
}
```

#### `src/main.rs` — dispatch to turn::run

```rust
Command::Run => {
    let engine = Engine::default();
    turn::run(&engine, &ur_root, &workspace)?;
}
```

#### `scripts/smoke-test.sh` — add ur run test

Add a section that enables test-extension, runs `ur run`, and checks that output contains expected debug lines.

## References

### Internal

- [extension_host.rs](src/extension_host.rs) — `ExtensionInstance` enum and provider methods
- [model.rs](src/model.rs) — `collect_provider_models`, `resolve_role`, `find_descriptor`
- [config.rs](src/config.rs) — `UserConfig::load`, `settings_for`
- [manifest.rs](src/manifest.rs) — `scan_and_load`, `ManifestEntry`
- [world.wit](wit/world.wit) — WIT interface definitions
- [llm-anthropic/src/lib.rs](extensions/user/llm-anthropic/src/lib.rs) — LLM stub
- [test-extension/src/lib.rs](extensions/workspace/test-extension/src/lib.rs) — tool-providing extension
