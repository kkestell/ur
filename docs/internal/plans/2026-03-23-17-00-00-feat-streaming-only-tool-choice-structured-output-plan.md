---
title: "feat: Streaming-only LLM interface with tool_choice and parallel tool dispatch"
type: feat
date: 2026-03-23
---

# Streaming-Only LLM Interface with tool_choice and Structured Output

## Overview

Unify the LLM provider contract to streaming-only, add `tool-choice` to the WIT, enable parallel tool dispatch, and unlock structured output via the tool-as-structured-output pattern. Three changes that build on each other in sequence.

## Problem Statement

1. **Two code paths:** `complete()` is already dead code (`#[expect(dead_code)]` in [extension_host.rs:433](src/extension_host.rs#L433)). Maintaining both streaming and non-streaming interfaces doubles the surface area for no benefit.
2. **No tool_choice:** Providers can't be told to force a specific tool or suppress tool use. This blocks structured output and fine-grained control.
3. **Sequential tool dispatch:** [turn.rs:274-304](src/turn.rs#L274-L304) dispatches tool calls one at a time. When the model emits multiple tool calls, they should execute in parallel.
4. **OpenRouter batches tool calls:** The OpenRouter extension accumulates tool calls in `pending_tool_calls` and emits them all at `finish_reason` ([llm-openrouter/src/lib.rs:308-325](extensions/system/llm-openrouter/src/lib.rs#L308-L325)). Extensions should emit each tool call as soon as it's fully assembled.

## Proposed Solution

Three phases executed in order. Each phase is independently shippable and testable.

### Phase 1: Streaming-only interface

Remove `complete()` from WIT. Rename `complete-streaming` to `complete`. All providers implement only the streaming path.

### Phase 2: tool_choice + eager tool emission + parallel dispatch

Add `tool-choice` variant to WIT. Update extensions to map it to provider APIs. Change OpenRouter to emit tool calls eagerly. Host dispatches tool calls in parallel after stream closes.

### Phase 3: Structured output (future — not in this plan)

Phase 2 primitives enable `tool_choice: specific(name)` which is the structured output pattern. Phase 3 adds host-side ergonomic helpers. Deferred to a separate plan.

## Technical Approach

### Phase 1: Streaming-only

**WIT changes** ([wit/world.wit](wit/world.wit)):

```wit
// REMOVE:
//   interface llm-provider { complete: func(...) -> result<completion, string>; }
//   world llm-extension exports llm-provider

// CHANGE: rename complete-streaming to complete
interface llm-streaming-provider {
    complete: func(messages: list<message>, model: string,
        settings: list<config-setting>, tools: list<tool-descriptor>)
        -> result<completion-stream, string>;
}
```

Decision: keep the interface name `llm-streaming-provider` or rename to `llm-provider`? The brainstorm says rename. Since the non-streaming interface is being removed, rename the interface to `llm-provider` and the world to `llm-extension`. This is a clean break.

**Files affected:**

| File | Change |
|------|--------|
| [wit/world.wit](wit/world.wit) | Remove `llm-provider` interface and its `complete` func. Rename `llm-streaming-provider` to `llm-provider`. Rename `complete-streaming` to `complete`. Remove old `llm-extension` world. Rename `llm-streaming-extension` world to `llm-extension`. |
| [src/extension_host.rs](src/extension_host.rs) | Remove `complete()` method (~L437-460). Update `bindgen!` macros for renamed world/interface. Rename `complete_streaming()` to `complete()`. |
| [src/turn.rs](src/turn.rs) | Update call sites from `complete_streaming` to `complete`. |
| [src/slot.rs](src/slot.rs) | Update slot detection to look for new interface name if the export name changes. |
| [extensions/system/llm-google/src/lib.rs](extensions/system/llm-google/src/lib.rs) | Remove `impl LlmGuest`. Rename `LlmStreamingGuest` impl. Update `complete_streaming` to `complete`. |
| [extensions/system/llm-openrouter/src/lib.rs](extensions/system/llm-openrouter/src/lib.rs) | Same as Google. |
| [extensions/workspace/llm-test/src/lib.rs](extensions/workspace/llm-test/src/lib.rs) | Same — remove non-streaming impl, rename streaming. |

### Phase 2: tool_choice + eager emission + parallel dispatch

**WIT changes:**

```wit
variant tool-choice {
    auto,
    none,
    required,
    specific(string),
}

// Updated signature
interface llm-provider {
    complete: func(messages: list<message>, model: string,
        settings: list<config-setting>, tools: list<tool-descriptor>,
        tool-choice: option<tool-choice>)
        -> result<completion-stream, string>;
}
```

`option<tool-choice>` — `None` means don't send tool_choice to the provider (use provider default). `Some(auto)` explicitly sends auto.

**Provider mapping:**

| WIT variant | OpenAI/OpenRouter | Gemini |
|-------------|------------------|--------|
| `auto` | `"auto"` | `mode: "AUTO"` |
| `none` | `"none"` | `mode: "NONE"` |
| `required` | `"required"` | `mode: "ANY"` |
| `specific(name)` | `{"type":"function","function":{"name":"X"}}` | `mode: "ANY"` + `allowedFunctionNames: ["X"]` |

**OpenRouter eager tool emission:**

Currently [llm-openrouter/src/lib.rs](extensions/system/llm-openrouter/src/lib.rs) accumulates deltas in `pending_tool_calls` and emits all on `finish_reason`. Change to:
- Continue accumulating deltas by index (OpenAI streams tool call args in fragments — accumulation is required)
- When a tool call's arguments JSON is complete (valid JSON parse succeeds), emit it immediately as a `MessagePart::ToolCall` chunk
- Remove the batch emission at `finish_reason`

Detecting completeness: try `serde_json::from_str` on the accumulated arguments after each delta append. When it parses, the tool call is complete. This is the standard approach — arguments are streamed as partial JSON strings, and a successful parse indicates completeness.

**Parallel tool dispatch in host:**

[turn.rs:274-304](src/turn.rs#L274-L304) currently loops sequentially. Change to:
- Accumulate all tool calls from the stream into a Vec
- After stream closes, spawn a `std::thread::scope` with one thread per tool call
- Each thread calls the extension's `call_tool(name, args_json)`
- Collect results via `JoinHandle`s
- No async runtime, no channels — just scoped threads

**Files affected:**

| File | Change |
|------|--------|
| [wit/world.wit](wit/world.wit) | Add `tool-choice` variant. Add `tool-choice: option<tool-choice>` parameter to `complete`. |
| [src/extension_host.rs](src/extension_host.rs) | Thread `tool_choice` through `complete()` call. |
| [src/turn.rs](src/turn.rs) | Pass `None` for tool_choice (default). Refactor `dispatch_tool_calls` to use `std::thread::scope` for parallel execution. |
| [extensions/system/llm-openrouter/src/lib.rs](extensions/system/llm-openrouter/src/lib.rs) | Map `tool_choice` to OpenAI format in request body. Change streaming to emit tool calls eagerly via JSON parse check. |
| [extensions/system/llm-google/src/lib.rs](extensions/system/llm-google/src/lib.rs) | Map `tool_choice` to Gemini `toolConfig.functionCallingConfig` in request body. Already emits tools eagerly — no streaming change. |
| [extensions/workspace/llm-test/src/lib.rs](extensions/workspace/llm-test/src/lib.rs) | Accept `tool_choice` parameter. When `specific(name)` is passed, emit a tool call for that specific tool with well-formed JSON args. |

## Acceptance Criteria

### Phase 1: Streaming-only

- [x] `complete()` (non-streaming) removed from WIT
- [x] `complete-streaming` renamed to `complete` in WIT
- [x] `llm-streaming-provider` renamed to `llm-provider`; old `llm-provider` interface removed
- [x] All three LLM extensions (Google, OpenRouter, llm-test) updated to new signature
- [x] Host `extension_host.rs` only has one `complete` method (streaming)
- [x] `make verify` passes
- [ ] `make smoke-test` passes

### Phase 2: tool_choice + eager emission + parallel dispatch

- [x] `tool-choice` variant type in WIT with `auto | none | required | specific(string)`
- [x] `complete` signature includes `tool-choice: option<tool-choice>`
- [x] OpenRouter maps tool_choice to OpenAI API format in request body
- [x] Google maps tool_choice to Gemini `toolConfig.functionCallingConfig` format
- [x] OpenRouter emits tool calls eagerly (JSON parse check on accumulated arguments)
- [x] `dispatch_tool_calls` uses `std::thread::scope` for parallel execution
- [x] llm-test responds to `tool_choice: specific(name)` with appropriate tool call
- [x] `make verify` passes
- [ ] `make smoke-test` passes with tool_choice plumbing validated

## Smoke Testing Strategy

### Phase 1
Existing smoke tests should pass after rename — they already use the streaming path.

### Phase 2
Update llm-test to exercise `tool_choice`:
- When `tool_choice: specific("get_structured_weather")` is passed, return a tool call with well-formed JSON matching a known schema
- Validates tool_choice flows through host → extension → response correctly
- Parallel dispatch tested by having the agent turn produce multiple tool calls

## Dependencies & Risks

- **WIT rename ripple:** Renaming interfaces changes the export names that `slot.rs` uses for slot detection. Must update the string match in [slot.rs](src/slot.rs).
- **bindgen! macro regeneration:** The `wasmtime::component::bindgen!` macros in [extension_host.rs](src/extension_host.rs) reference world names. Renaming worlds requires updating all four `bindgen!` invocations.
- **OpenRouter eager emission correctness:** The JSON parse check for argument completeness must handle edge cases (e.g., arguments that are valid JSON prefixes of the final value). In practice this is safe — OpenAI streams arguments as string fragments, and a complete JSON object won't be a prefix of a longer valid JSON object.
- **Thread safety for parallel dispatch:** `ExtensionInstance` holds a `Store<HostState>` which is not `Send`. Parallel dispatch may require one extension instance per thread or serialized access to the store. Investigate whether `call_tool` needs mutable store access — if so, parallel dispatch across different extensions is fine but two tools on the same extension need serialization or cloning.

## References

- Brainstorm: [.agents/brainstorms/2026-03-23-16-42-31-streaming-tool-choice-structured-output-brainstorm.md](.agents/brainstorms/2026-03-23-16-42-31-streaming-tool-choice-structured-output-brainstorm.md)
- Prior plan (multi-part messages): [.agents/plans/2026-03-22-13-42-30-feat-self-describing-extensions-and-multi-part-messages-plan.md](.agents/plans/2026-03-22-13-42-30-feat-self-describing-extensions-and-multi-part-messages-plan.md)
- Prior plan (OpenRouter): [.agents/plans/2026-03-22-20-29-56-feat-openrouter-provider-and-dynamic-catalog-plan.md](.agents/plans/2026-03-22-20-29-56-feat-openrouter-provider-and-dynamic-catalog-plan.md)
