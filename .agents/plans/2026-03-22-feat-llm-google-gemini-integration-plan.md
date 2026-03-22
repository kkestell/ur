---
title: "feat: Real LLM integration via Google Gemini"
type: feat
date: 2026-03-22
---

# Real LLM integration via Google Gemini

## Overview

Replace the stub LLM extensions with a real Google Gemini provider that makes actual API calls. This is the first real LLM in the system — it proves out WASI HTTP in extensions, streaming completions, and tool use end-to-end. Builds on the brainstorm at `.agents/brainstorms/2026-03-22-llm-google-streaming-brainstorm.md`.

## Problem Statement / Motivation

Every LLM extension is a hardcoded stub. The turn loop works, but it exercises fake data. To validate the architecture for real use, we need at least one provider making real HTTP calls, parsing real responses, and handling real tool calls — all from inside the WASM sandbox.

Google Gemini is the starting point because the user has a `GOOGLE_API_KEY` ready.

## Exploratory Findings

### Gemini REST API (verified 2026-03-22)

**Base URL:** `https://generativelanguage.googleapis.com/v1beta/models/{model}`

**Endpoints:**

| Operation | URL |
|---|---|
| Non-streaming | `POST .../models/{model}:generateContent` |
| Streaming | `POST .../models/{model}:streamGenerateContent?alt=sse` |

**Authentication:** `x-goog-api-key: {key}` header

**Request body:**
```json
{
  "systemInstruction": {
    "parts": [{ "text": "You are a helpful assistant." }]
  },
  "contents": [
    {
      "role": "user",
      "parts": [{ "text": "Hello" }]
    },
    {
      "role": "model",
      "parts": [{ "text": "Hi there!" }]
    }
  ],
  "tools": [{
    "functionDeclarations": [{
      "name": "get_weather",
      "description": "Gets current weather",
      "parameters": {
        "type": "object",
        "properties": {
          "location": { "type": "string" }
        },
        "required": ["location"]
      }
    }]
  }],
  "generationConfig": {
    "temperature": 0.7,
    "maxOutputTokens": 8192
  }
}
```

**SSE response chunks:** Each `data:` line is a full `GenerateContentResponse`:
```json
{
  "candidates": [{
    "content": {
      "role": "model",
      "parts": [{ "text": "The weather" }]
    },
    "finishReason": "STOP",
    "index": 0
  }],
  "usageMetadata": {
    "promptTokenCount": 25,
    "candidatesTokenCount": 42,
    "totalTokenCount": 67
  }
}
```

**Function call response:**
```json
{
  "candidates": [{
    "content": {
      "role": "model",
      "parts": [{
        "functionCall": {
          "name": "get_weather",
          "id": "8f2b1a3c",
          "args": { "location": "Seattle" }
        }
      }]
    },
    "finishReason": "TOOL_USE"
  }]
}
```

**Function result (sent as `role: "user"`):**
```json
{
  "role": "user",
  "parts": [{
    "functionResponse": {
      "name": "get_weather",
      "id": "8f2b1a3c",
      "response": { "temperature": "55F", "conditions": "rainy" }
    }
  }]
}
```

**Key differences from OpenAI/Anthropic:**
- System prompt is top-level `systemInstruction`, not a message in `contents`
- Role is `"model"` not `"assistant"`
- Tool results are `functionResponse` parts in `role: "user"`, not `role: "tool"`
- `functionCall.args` is a JSON object, not a string
- `functionCall.id` required for Gemini 3.x, may be absent in older models
- Finish reason `"TOOL_USE"` signals function calls
- Multiple `functionCall` parts can appear in a single response (parallel tool calls)
- JSON uses camelCase; API accepts both camelCase and snake_case input

**Current production models:** `gemini-3-flash-preview` (default), `gemini-3-pro-preview`

### WIT Resources (verified 2026-03-22)

WIT resources are **fully supported and stable** in wasmtime 43 + wit-bindgen 0.54.

**Host-side pattern:**
```rust
// 1. Call complete-streaming to get resource handle
let stream: ResourceAny = bindings
    .ur_extension_llm_streaming_provider()
    .call_complete_streaming(&mut store, &messages, model, &settings)?;

// 2. Get resource method accessor
let stream_methods = bindings
    .ur_extension_llm_streaming_provider()
    .completion_stream();

// 3. Pull chunks
loop {
    match stream_methods.call_next(&mut store, stream)? {
        Some(chunk) => { /* process */ }
        None => break,
    }
}

// 4. Drop resource (REQUIRED — does not auto-drop)
stream.resource_drop(&mut store)?;
```

**Guest-side pattern:**
```rust
impl Guest for MyComponent {
    type CompletionStream = MyStream;
    fn complete_streaming(...) -> Result<CompletionStream, String> {
        CompletionStream::new(MyStream { ... })
    }
}

// Methods receive &self only — must use RefCell for mutable state
impl GuestCompletionStream for MyStream {
    fn next(&self) -> Option<CompletionChunk> {
        self.inner.borrow_mut().next_chunk()
    }
}
```

**Gotchas:**
- Guest resource methods are `&self` only — use `RefCell` for mutable state
- Host must call `resource_drop()` manually or resource leaks until Store drops
- `ResourceAny` is `Copy` — same handle passed to each `call_next()`

**Verdict:** No fallback needed. Resources are the right tool here.

## Proposed Solution

Three layers of work:

1. **Enable WASI HTTP** in the host so WASM extensions can make outbound HTTP requests
2. **Add streaming WIT interface** with a resource-based pull model (all providers must implement)
3. **Build `llm-google` extension** that calls the Gemini API with streaming + tool use

### Architecture

```
Host (Rust binary)
  │
  ├─ wasmtime engine
  │   ├─ wasmtime-wasi (stdio, clocks, io) ← existing
  │   └─ wasmtime-wasi-http (outgoing HTTP) ← NEW
  │
  ├─ extension_host.rs
  │   ├─ HostState { wasi_ctx, http_ctx, resource_table } ← http_ctx NEW
  │   ├─ WasiView impl ← existing
  │   └─ WasiHttpView impl ← NEW
  │
  └─ turn.rs
      └─ calls complete_streaming() → pulls chunks via next() ← NEW

Extension (WASM component)
  └─ llm-google
      ├─ imports wasi:http/outgoing-handler ← NEW
      ├─ exports llm-provider (provider-id, list-models, complete)
      └─ exports llm-streaming-provider (complete-streaming → resource) ← NEW
```

## Technical Approach

### Phase 1: Enable WASI HTTP in the Host

**Goal:** WASM extensions can make outbound HTTPS requests.

**Host dependency:**
- Add `wasmtime-wasi-http = "43.0.0"` (matches existing wasmtime version)

**HostState changes** (`src/extension_host.rs`):
- Add `http_ctx: WasiHttpCtx` field
- Implement `WasiHttpView` trait (shares the same `ResourceTable` as `WasiView`)
- In linker setup for LLM extensions, add: `wasmtime_wasi_http::p2::add_only_http_to_linker_sync(&mut linker)?`
- Use `add_only_http_to_linker_sync` (NOT `add_to_linker_sync`) to avoid duplicating base WASI interfaces

**WIT vendoring:**
- Vendor `wasi:http@0.2.0` WIT files into `wit/deps/` (types.wit, handler.wit, proxy.wit)
- These depend on `wasi:io` and `wasi:clocks` which wasmtime-wasi already provides

**Selective enablement:** Only add HTTP to the linker for `llm-extension` world (providers that call external APIs). Session, compaction, and general extensions don't need it.

#### Tasks

- [x] `cargo add wasmtime-wasi-http@43.0.0` in host Cargo.toml
- [x] Vendor wasi:http WIT files into `wit/deps/`
- [x] Add `WasiHttpCtx` to `HostState`, implement `WasiHttpView`
- [x] Call `add_only_http_to_linker_sync` in LLM extension linker setup
- [x] ~~Verify existing stubs still compile and load~~ (stubs deleted per user direction)

### Phase 2: Streaming WIT Interface

**Goal:** All LLM providers return completion chunks incrementally via a resource. Streaming is mandatory for all providers.

**New WIT types** (in `wit/world.wit`):

```wit
record completion-chunk {
    delta-text: option<string>,
    delta-tool-calls: list<tool-call>,
    finish-reason: option<string>,
    usage: option<usage>,
}

resource completion-stream {
    next: func() -> option<completion-chunk>;
}
```

**New interface** `llm-streaming-provider`:

```wit
interface llm-streaming-provider {
    use types.{message, config-setting, completion-chunk, completion-stream};

    complete-streaming: func(messages: list<message>, model: string, settings: list<config-setting>)
        -> result<completion-stream, string>;
}
```

**Updated `llm-extension` world:**

```wit
world llm-extension {
    import host;
    import wasi:http/outgoing-handler@0.2.0;
    export extension;
    export llm-provider;
    export llm-streaming-provider;
}
```

**Host-side consumption** (`src/extension_host.rs`):
- Add `complete_streaming()` method to `ExtensionInstance`
- Returns chunks via the `ResourceAny` + `GuestCompletionStream` accessor pattern
- Must call `resource_drop()` after consuming all chunks
- Update `bindgen!` macro for `llm` world with the new interface and resource

**Turn loop update** (`src/turn.rs`):
- Call `complete_streaming()` instead of `complete()`
- Pull chunks in a loop, print deltas to stdout as they arrive
- Assemble final `Completion` from accumulated chunks
- Keep `complete()` as an existing interface but prefer streaming

**Guest implementation note:** Resource methods are `&self` only. Mutable streaming state (HTTP response buffer, parse position) must use `RefCell`.

#### Tasks

- [x] Add `completion-chunk` record and `completion-stream` resource to WIT types
- [x] Add `llm-streaming-provider` interface to WIT
- [x] Update `llm-extension` world with new interface + `wasi:http/outgoing-handler` import
- [x] Update host-side `bindgen!` and `ExtensionInstance` for streaming
- [x] Update turn loop to consume stream chunks with incremental output
- [x] ~~Update existing stubs~~ (stubs deleted per user direction)

### Phase 3: Google Gemini Extension

**Goal:** Real API calls to Google Gemini with streaming and tool use.

**Extension structure:**

```
extensions/system/llm-google/
├── Cargo.toml          (wit-bindgen + serde + serde_json)
├── extension.toml      (id: llm-google, slot: llm-provider)
└── src/lib.rs          (implementation)
```

**`extension.toml`:**

```toml
[extension]
id = "llm-google"
name = "LLM Google Gemini"
slot = "llm-provider"
wasm = "target/wasm32-wasip2/release/llm_google.wasm"
```

**API key handling:**
- Host reads `GOOGLE_API_KEY` from environment
- Passes to extension via `init(config)` as `("api_key", "<value>")`
- Extension stores in a `thread_local!` `RefCell<Option<String>>` for use in `complete()`/`complete_streaming()`

**Models:**

| Model ID | Name | Default | Settings |
|---|---|---|---|
| `gemini-3-flash-preview` | Gemini 2.5 Flash | Yes | `temperature` (int 0–200, represents 0.0–2.0 × 100), `max_output_tokens` (int 1–65536) |
| `gemini-3-pro-preview` | Gemini 2.5 Pro | No | `temperature`, `max_output_tokens` |

**Gemini API mapping (verified):**

| ur concept | Gemini REST API |
|---|---|
| `messages` with role `"user"` | `contents` entry with `"role": "user"`, `"parts": [{"text": "..."}]` |
| `messages` with role `"assistant"` | `contents` entry with `"role": "model"`, `"parts": [{"text": "..."}]` |
| `messages` with role `"system"` | Top-level `"systemInstruction": {"parts": [{"text": "..."}]}` (extracted from messages, not in contents) |
| `messages` with role `"tool"` | `contents` entry with `"role": "user"`, `"parts": [{"functionResponse": {"name": "...", "id": "...", "response": {...}}}]` |
| `tool_calls` in Completion | `"parts": [{"functionCall": {"name": "...", "id": "...", "args": {...}}}]` in model response |
| `finish_reason` `"TOOL_USE"` | Model wants function execution |
| streaming | `POST :streamGenerateContent?alt=sse`, each `data:` line is full `GenerateContentResponse` |
| auth | `x-goog-api-key: {key}` header |

**HTTP via WASI** (guest-side pattern):
1. Build `OutgoingRequest` with headers, method, authority, path+query
2. Write JSON body via `OutgoingBody` stream
3. Send via `outgoing_handler::handle()`
4. For streaming: read `IncomingBody` stream incrementally, parse SSE `data:` lines
5. Each parsed chunk → `completion-chunk` returned from `next()`
6. State held in `RefCell` inside the resource (required because methods are `&self`)

**Tool call handling:**
- Gemini returns `functionCall` parts with `name`, `id`, and `args` (JSON object)
- Map to `ToolCall { id: functionCall.id, name: functionCall.name, arguments_json: serde_json::to_string(functionCall.args) }`
- Multiple `functionCall` parts can appear in one response (parallel tool calls)
- Tool results come back as `functionResponse` parts in a `role: "user"` message
- `functionResponse.id` must match the original `functionCall.id`

**Error handling:**
- HTTP non-2xx → `Err(format!("Gemini API error: HTTP {status}"))`
- JSON parse failure → `Err("failed to parse Gemini response: ...")`
- Missing API key → `Err("GOOGLE_API_KEY not configured")` from `init()`

#### Tasks

- [x] Create `extensions/system/llm-google/` with Cargo.toml, extension.toml, src/lib.rs
- [x] Implement `ExtGuest` (init with API key storage in thread_local RefCell, call_tool returns error)
- [x] Implement `LlmGuest` (provider_id, list_models, complete via non-streaming endpoint)
- [x] Implement `LlmStreamingGuest` (complete_streaming with SSE parsing, RefCell-based resource state)
- [x] Build message format conversion (ur messages → Gemini contents + systemInstruction extraction)
- [x] Build tool call parsing (Gemini functionCall → ur ToolCall, including parallel calls)
- [x] Build tool result formatting (ur tool message → Gemini functionResponse with id matching)
- [x] SSE stream parser for `data:` lines from `?alt=sse` endpoint

### Phase 4: Host-Side Plumbing

**Goal:** Wire everything together so `ur run` hits the real API.

**API key injection:**
- Host reads `GOOGLE_API_KEY` from `std::env::var()`
- Convention: `{PROVIDER_ID_UPPER}_API_KEY` env var → passed as `("api_key", value)` to that provider's `init(config)`
- For google: `GOOGLE_API_KEY` → `("api_key", value)` to llm-google

**Smoke test updates** (`scripts/smoke-test.sh`):
- Add llm-google to the build list
- Add extension discovery/enable tests for llm-google
- Mark real-API tests as optional (skip if no `GOOGLE_API_KEY`)
- Add a simple integration test: `GOOGLE_API_KEY=... ur run` and verify real output

#### Tasks

- [x] Add env var reading for API keys in host, pass via init config
- [x] Update smoke test to build and discover llm-google
- [x] Add optional real-API integration test gated on GOOGLE_API_KEY presence

## Acceptance Criteria

- [x] `ur extensions list` shows llm-google as a system extension, enabled by default
- [x] `ur model list` shows gemini-3-flash-preview and gemini-3-pro-preview
- [x] `ur run` with `GOOGLE_API_KEY` set produces a real streamed response from Gemini
- [x] Streaming output appears incrementally on stdout
- [ ] Tool calls from Gemini are dispatched to the test-extension and results sent back (needs live test)
- [ ] The full turn loop works: user message → LLM → tool call → tool result → LLM → final response (needs live test)
- [x] ~~Existing stubs~~ (deleted per user direction — not needed)
- [x] No API key → clear error message, not a crash

## Dependencies & Risks

| Risk | Mitigation | Status |
|---|---|---|
| WIT resources may have bindgen/wasmtime quirks | Verified: resources fully supported in wasmtime 43 + wit-bindgen 0.54. Guest methods are `&self` only (use RefCell). Host must call `resource_drop()`. | **Resolved** |
| WASI HTTP may not support streaming reads cleanly | Can buffer full response and return chunks from parsed buffer (degrade gracefully) | Open |
| Gemini API shape unknown (post-training-cutoff) | Verified: REST API shape documented above, camelCase JSON, SSE via `?alt=sse` | **Resolved** |
| serde_json in WASM may bloat binary size | Acceptable for now; optimize later if needed | Accepted |
| wasmtime-wasi-http pulls in hyper/rustls/tokio transitive deps | Only affects host binary size, not extension size | Accepted |

## References

- Brainstorm: `.agents/brainstorms/2026-03-22-llm-google-streaming-brainstorm.md`
- Existing LLM stub pattern: `extensions/system/llm-openai/src/lib.rs`
- Extension host: `src/extension_host.rs`
- Turn loop: `src/turn.rs`
- WIT definitions: `wit/world.wit`
- wasmtime-wasi-http docs: https://docs.rs/wasmtime-wasi-http/43.0.0
- Gemini API text generation: https://ai.google.dev/gemini-api/docs/text-generation
- Gemini API function calling: https://ai.google.dev/gemini-api/docs/function-calling
- Gemini API overview: https://ai.google.dev/gemini-api/docs/api-overview
- Gemini models: https://ai.google.dev/gemini-api/docs/models
