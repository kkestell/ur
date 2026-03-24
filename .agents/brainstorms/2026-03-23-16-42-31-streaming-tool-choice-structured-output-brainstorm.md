# Streaming-Only LLM Interface with tool_choice and Structured Output

## What We're Building

Three interrelated changes to the LLM provider WIT interface and host orchestration:

1. **Streaming-only interface** — Remove the non-streaming `complete()` path. Make `complete-streaming` the sole contract, renamed to `complete`. Non-streaming providers emit all chunks at once after their HTTP response.

2. **Eager tool dispatch with tool_choice** — Add a `tool-choice` variant to the WIT (`auto | none | required | specific(tool-name)`). Extensions emit each tool call as soon as it's fully received. The host spawns tool execution immediately during the stream, overlapping with remaining LLM output.

3. **Structured output support** — The `tool_choice: specific(name)` primitive enables the "tool-as-structured-output" pattern. Phase 1 uses raw primitives; Phase 2 adds a host-side ergonomic helper.

## Why This Approach

**Streaming-only:** The non-streaming `complete()` is already dead code on the host. Two code paths means two things to test and maintain. A non-streaming provider can trivially implement the streaming interface by emitting all events after its HTTP response completes. One path, universally.

**Eager tool emission + parallel dispatch:** The current OpenRouter extension batches all tool calls until `finish_reason`. Extensions should instead emit each tool call as soon as it's fully received. The host accumulates them during the stream, then dispatches all in parallel after the stream closes. This is the standard pattern used by Claude Code and Cursor. The ReAct loop waits for ALL tool results before the next LLM turn — the win is parallel tool execution (tool 1 and tool 2 execute concurrently).

**tool_choice as a WIT variant:** Typed, extensible, and covers all provider patterns (Anthropic, OpenAI, Google all support auto/none/required/specific). This is the primitive that unlocks structured output.

**Structured output via tool-as-structured-output:** Instead of adding a separate "structured output" API, we use the existing tool machinery with `tool_choice: specific(name)`. Define a fake tool with a JSON schema, force the model to use it, parse the result. Works across all providers. Phase 1 = raw primitives, Phase 2 = host-side helper for ergonomics.

## Key Decisions

1. **Streaming-only:** Remove `complete()` from WIT entirely. Rename `complete-streaming` to `complete`.
2. **Eager tool emission:** Extensions MUST emit each tool call as a complete `message-part::tool-call` as soon as the full name + arguments JSON are available. No batching.
3. **tool_choice variant:** `auto | none | required | specific(string)` — added as a new parameter to `complete`.
4. **Parallel dispatch after stream:** Accumulate tool calls during streaming, dispatch all in parallel after stream closes. Text still streams to display immediately. This is the standard pattern (Claude Code, Cursor, etc.) — the latency delta vs during-stream dispatch is negligible since the LLM stream tail after tool calls is typically just a finish event.
5. **No async runtime needed:** Threads + JoinHandles (or rayon), not channels or async. Simplest approach.
6. **Structured output Phase 1:** Extensions use `complete` + `tool_choice: specific("my_tool")` directly. No special abstraction yet.
7. **Structured output Phase 2 (future):** Host-side helper that takes a JSON schema, creates the fake tool descriptor, calls `complete` with forced tool choice, and returns parsed structured data.

## WIT Interface Changes

### Before

```wit
complete: func(messages: list<message>, model: string,
    settings: list<config-setting>, tools: list<tool-descriptor>)
    -> result<completion, string>;

complete-streaming: func(messages: list<message>, model: string,
    settings: list<config-setting>, tools: list<tool-descriptor>)
    -> result<completion-stream, string>;
```

### After

```wit
variant tool-choice {
    auto,
    none,
    required,
    specific(string),
}

complete: func(messages: list<message>, model: string,
    settings: list<config-setting>, tools: list<tool-descriptor>,
    tool-choice: option<tool-choice>)
    -> result<completion-stream, string>;
```

## Impact

### Extensions affected
- **llm-google:** Update to new `complete` signature, pass `tool_choice` to Gemini API (`toolConfig.functionCallingConfig.mode`). Already emits tools eagerly — no streaming behavior change.
- **llm-openrouter:** Update to new `complete` signature, pass `tool_choice` to OpenAI API. Change streaming to emit tool calls eagerly instead of batching in `pending_tool_calls`.
- **Future providers:** Implement only the streaming `complete` function with `tool_choice` support.

### Host affected
- **extension_host.rs:** Remove `complete()` host binding. Update `complete_streaming` to pass `tool_choice`.
- **turn.rs:** Refactor `dispatch_tool_calls` to execute tools in parallel (scoped threads or rayon). Accumulate tool calls during stream, dispatch after stream closes.
- **compaction-llm extension:** If it uses the LLM interface internally, it needs updating too.

## Smoke Testing Strategy

### Layer 1: Deterministic (llm-test extension)
Update llm-test to exercise the new `tool_choice` parameter:
- When `tool_choice: specific("get_structured_weather")` is passed, return a tool call with well-formed JSON matching a known schema.
- Tests the full WIT plumbing end-to-end without hitting a real API.
- Validates that tool_choice flows through host → extension → response correctly.

### Layer 2: Live LLM (test-extension + real providers)
Add a new tool to test-extension (e.g., `get_structured_weather`) that:
- Internally uses `complete` with `tool_choice: specific(...)` against the active LLM provider.
- Passes a JSON schema as the fake tool definition (e.g., structured weather data).
- Returns the structured result to the agent.

The smoke test tells the agent to call this tool using both Gemini and OpenRouter, validating the tool-as-structured-output pattern works with real APIs end-to-end.

## Provider tool_choice Mapping

All three target providers support all four modes:

| WIT variant | OpenAI/OpenRouter | Gemini | Anthropic |
|-------------|------------------|--------|-----------|
| `auto` | `"auto"` | `mode: "AUTO"` | `{"type": "auto"}` |
| `none` | `"none"` | `mode: "NONE"` | `{"type": "none"}` |
| `required` | `"required"` | `mode: "ANY"` | `{"type": "any"}` |
| `specific(name)` | `{"type":"function","function":{"name":"X"}}` | `mode: "ANY"` + `allowedFunctionNames: ["X"]` | `{"type":"tool","name":"X"}` |

**Provider-specific constraints** (not modeled in WIT — extensions return errors):
- Anthropic: `specific` and `required` are incompatible with extended thinking. Only `auto` and `none` work when thinking is enabled.

## Resolved Questions

1. **tool-choice optionality:** `option<tool-choice>`. None = don't send tool_choice to the provider at all (provider uses its default). Some(auto) explicitly sends "auto". This matters because some providers behave differently when tool_choice is absent vs explicit.

2. **Phase 2 structured output helper location:** Host-side only. An imported WIT function like `structured-complete(schema, messages, model, settings)` that extensions can call. Extensions don't need to know the tool-as-structured-output trick — the host handles it.

3. **Error handling during eager dispatch:** Continue the stream; report the error as tool-result content. The LLM sees the error in the next turn and can retry or adapt. Stream is never cancelled due to tool failure. This is standard agentic behavior.
