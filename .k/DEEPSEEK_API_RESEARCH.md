# DeepSeek API Research Findings

Source of truth: https://api-docs.deepseek.com/ (fetched 2026-06-17)

Key pages consulted:
- [Quick Start / Your First API Call](https://api-docs.deepseek.com/)
- [Models & Pricing](https://api-docs.deepseek.com/quick_start/pricing)
- [Error Codes](https://api-docs.deepseek.com/quick_start/error_codes)
- [Rate Limit & Isolation](https://api-docs.deepseek.com/quick_start/rate_limit)
- [Thinking Mode](https://api-docs.deepseek.com/guides/thinking_mode)
- [Tool Calls](https://api-docs.deepseek.com/guides/tool_calls)
- [JSON Output](https://api-docs.deepseek.com/guides/json_mode)
- [Context Caching](https://api-docs.deepseek.com/guides/kv_cache)
- [Chat Prefix Completion (Beta)](https://api-docs.deepseek.com/guides/chat_prefix_completion)
- [FIM Completion (Beta)](https://api-docs.deepseek.com/guides/fim_completion)
- [API Reference: Chat Completion](https://api-docs.deepseek.com/api/create-chat-completion)
- [API Reference: List Models](https://api-docs.deepseek.com/api/list-models)

---

## 1. Client construction & auth

**Canonical base URL:** `https://api.deepseek.com`
— There is NO `/v1` prefix. The OpenAI SDK example uses `base_url="https://api.deepseek.com"` directly.
— The curl example calls `https://api.deepseek.com/chat/completions` (not `/v1/chat/completions`).
— **Divergence from OpenAI:** `/v1` is not part of the URL structure. A client constructed to always prepend `/v1` would break unless the server silently accepts both (untested, but the docs only show the pathless base).

**Beta base URL:** `https://api.deepseek.com/beta`
— Gates: Chat Prefix Completion, FIM Completion, strict-mode tool calls (`"strict": true` in tool definitions).
— Same auth scheme applies.

**Auth scheme:** `Authorization: Bearer <api_key>`
— No org/project headers documented.
— Optional `user_id` field in the JSON body (regex `[a-zA-Z0-9\-_]+`, max 512 chars). Used for content safety isolation, KVCache isolation, and scheduling isolation. Passed via `{"user_id": "..."}` in the request body (or `extra_body={"user_id": "..."}` in OpenAI SDK, or `metadata: {"user_id": "..."}` in Anthropic format).

**Timeouts & keep-alive:**
— Server sends keep-alive signals during idle periods: empty lines for non-stream, SSE comments (`: keep-alive`) for streaming.
— If inference has not started after **10 minutes**, the server closes the connection.
— No documented client timeout recommendation.

**Concurrency limits:**
| Model | Limit |
|-------|-------|
| `deepseek-v4-pro` | 500 |
| `deepseek-v4-flash` | 2500 |
— Limits are per-account (all API keys combined). Exceeding returns HTTP 429.
— Capacity expansion available via request form (no extra cost).

### Decisions for `ur`
- `DeepseekClient::builder()` should default `base_url` to `"https://api.deepseek.com"` (no `/v1`).
- Expose `beta_base_url` or a `beta()` builder toggle that switches the base to the `/beta` variant.
- Support `user_id` as an optional builder/config parameter.
- Default connect/read timeout should be generous (at least 10 min + buffer); streaming keep-alive parsing must handle `: keep-alive` comments.

---

## 2. Model catalog

### Current model IDs

| ID | Description | Deprecation |
|----|-------------|-------------|
| `deepseek-v4-flash` | Faster/cheaper variant, supports both thinking and non-thinking modes | — |
| `deepseek-v4-pro` | Highest quality variant | — |
| `deepseek-chat` | Maps to `deepseek-v4-flash` non-thinking mode | **2026-07-24** |
| `deepseek-reasoner` | Maps to `deepseek-v4-flash` thinking mode | **2026-07-24** |

### Per-model specs

| Property | `deepseek-v4-flash` | `deepseek-v4-pro` |
|----------|---------------------|-------------------|
| Context window | 1M tokens | 1M tokens |
| Max output (max\_tokens) | 384K | 384K |
| Default max\_tokens | not documented | not documented |
| Thinking mode | yes (both modes) | yes (both modes) |
| Thinking default | not documented | `enabled` |
| Tool/function calling | yes | yes |
| JSON output | yes | yes |
| Chat Prefix Completion (Beta) | yes | yes |
| FIM Completion (Beta) | non-thinking only | non-thinking only |
| Cache hit: 1M input tokens | $0.0028 | $0.003625 |
| Cache miss: 1M input tokens | $0.14 | $0.435 |
| 1M output tokens | $0.28 | $0.87 |

### `GET /models` endpoint
`GET https://api.deepseek.com/models` returns:
```json
{
  "object": "list",
  "data": [
    {"id": "deepseek-v4-flash", "object": "model", "owned_by": "deepseek"},
    {"id": "deepseek-v4-pro", "object": "model", "owned_by": "deepseek"}
  ]
}
```
— This endpoint exists and can be relied on for model discovery. It does **not** return context window or max output — those must be catalogued locally.

### Decisions for `ur`
- `Model::new("deepseek-v4-pro")` can fill `context_window: 1_000_000`, `max_output: 384_000` from a built-in table.
- The `context_window` and `max_output` fields are read-only (set by model id) — drop any user-settable `context_window` arg.
- Emit code that gates FIM/completions to non-thinking mode only.
- Warn on construction with deprecated IDs (`deepseek-chat`, `deepseek-reasoner`).

---

## 3. Chat request shape

**Endpoint:** `POST /chat/completions`

**Request body (all top-level fields):**

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `messages` | array | yes | min 1 item, roles: `system`, `user`, `assistant`, `tool` |
| `model` | string | yes | `"deepseek-v4-flash"` or `"deepseek-v4-pro"` |
| `thinking` | object \| null | no | `{"type": "enabled"/"disabled"}`, default: `enabled` for v4-pro (flash not doc'd) |
| `reasoning_effort` | string | no | `"high"` (default), `"max"`. Aliases: `low`/`medium` → `high`, `xhigh` → `max` |
| `max_tokens` | integer \| null | no | max 384K, default not doc'd |
| `temperature` | number \| null | no | 0–2, default 1. **Silently ignored in thinking mode** |
| `top_p` | number \| null | no | 0–1, default 1. **Silently ignored in thinking mode** |
| `presence_penalty` | — | — | **Deprecated globally** — passed but no effect |
| `frequency_penalty` | — | — | **Deprecated globally** — passed but no effect |
| `stop` | string \| string[] \| null | no | up to 16 sequences |
| `stream` | boolean \| null | no | |
| `stream_options` | object \| null | no | only used when `stream: true`. Has `include_usage: bool` |
| `response_format` | object \| null | no | `{"type": "json_object"}` for JSON mode. Default: `{"type": "text"}` |
| `tools` | object[] \| null | no | max 128 functions |
| `tool_choice` | string \| object \| null | no | `"none"`, `"auto"`, `"required"`, or `{"type":"function","function":{"name":"..."}}` |
| `logprobs` | boolean \| null | no | |
| `top_logprobs` | integer \| null | no | 0–20, requires `logprobs: true` |
| `user_id` | string \| null | no | regex `[a-zA-Z0-9-_]+`, max 512 |

**Message shapes:**

- **system:** `{"role": "system", "content": "<string>", "name": "<optional>"}`
- **user:** `{"role": "user", "content": "<string>", "name": "<optional>"}`
- **assistant:** `{"role": "assistant", "content": "<string|null>", "name": "<optional>", "prefix": "<bool>", "reasoning_content": "<string|null>", "tool_calls": "[...]"}` — `prefix` and `reasoning_content` are Beta (Chat Prefix Completion).
- **tool:** `{"role": "tool", "content": "<string>", "tool_call_id": "<string>"}` — content is always a string.

**JSON mode:** Request `response_format: {"type": "json_object"}`. Must also include the word "json" in system/user prompt and provide example format — otherwise model may emit infinite whitespace. May occasionally return empty content (known issue being optimized).

### DeepSeek-specific vs. OpenAI-compatible
- **DeepSeek-specific:** `thinking`, `reasoning_effort`, `user_id`, `frequency_penalty`/`presence_penalty` deprecated.
- **DeepSeek-specific:** assistant message carries `prefix` and `reasoning_content` fields (Beta).
- **Divergence:** `temperature`/`top_p` silently dropped in thinking mode instead of rejected.
- **Otherwise:** request body is OpenAI-compatible.

### Decisions for `ur`
- `Model`/`Session` should gate which params are sent based on thinking mode: omit `temperature`/`top_p` when thinking=on, or send but document they're ignored.
- `frequency_penalty` and `presence_penalty` should NOT be exposed in `ur` — they are deprecated.
- `response_format` should be an optional enum: `Text` (default) or `JsonObject`.
- Chat Prefix Completion (`prefix: true`) is Beta-only; require beta base URL.

---

## 4. Streaming (SSE)

**Activation:** Set `stream: true` in request body.

**SSE framing:**
- Each event: `data: <JSON>\n\n`
- Terminal sentinel: `data: [DONE]`
- Keep-alive: `: keep-alive` (SSE comment) during idle periods before inference starts.
- Empty lines during idle for non-streaming requests.

**Streaming chunk envelope (each `data:` line is one JSON object):**

```
{
  "id": "string",           // same ID across all chunks in one stream
  "object": "chat.completion.chunk",
  "created": 1718345013,    // same across all chunks
  "model": "deepseek-v4-pro",
  "system_fingerprint": "fp_...",
  "choices": [{
    "index": 0,
    "delta": {
      "role": "assistant",              // present in first chunk, null after
      "content": "Hello",               // text delta (null when reasoning/tool)
      "reasoning_content": "Let me...", // reasoning delta (thinking mode)
      "tool_calls": [...]               // tool call deltas
    },
    "finish_reason": null | "stop" | "length" | ...,
    "logprobs": null
  }],
  "usage": null | { ... }   // only in final chunk if stream_options.include_usage=true
}
```

**Where text content arrives:** `choices[0].delta.content` — each chunk contains a token or few tokens. String concatenation builds the full response.

**Where reasoning content arrives:** `choices[0].delta.reasoning_content` — same pattern as `content`, streamed separately. The two are mutually exclusive in a single chunk: a chunk carries EITHER `reasoning_content` OR `content`, never both (per the streaming example code in thinking mode docs that checks `if chunk.choices[0].delta.reasoning_content` as an else-branch).  
In the API reference schema, both fields are `nullable` strings on `delta` — when one is present the other is empty/null.

**`finish_reason` values:**

| Value | Meaning |
|-------|---------|
| `stop` | Natural stop or stop sequence hit |
| `length` | `max_tokens` or context length reached |
| `content_filter` | Content omitted due to content filters |
| `tool_calls` | Model called a tool |
| `insufficient_system_resource` | Interrupted due to insufficient inference resource |

**Streaming usage:** The `usage` object appears only in the final chunk, and ONLY when `stream_options: {"include_usage": true}` is set. Without this option, the final chunk has `usage: null`. The final chunk that carries usage has `choices: []` (empty array) per the API ref.

### Decisions for `ur`
- `Event::Done` should carry `finish_reason` as an enum: `Stop`, `Length`, `ContentFilter`, `ToolCalls`, `InsufficientSystemResource`.
- `Event` enum design is correct: `TextDelta(String)`, `ReasoningDelta(String)`, `ToolCallDelta { ... }`, `Done { finish_reason, usage }`.
- `stream_options: {"include_usage": true}` must be sent to get usage from streams — hardcode this.
- SSE parser must handle `: keep-alive` comments and empty lines gracefully.

---

## 5. Tool / function calling

### Request-side tool declaration

Exact JSON schema follows OpenAI format:

```json
{
  "type": "function",
  "function": {
    "name": "<string>",        // required, max 64 chars, [a-zA-Z0-9_-]
    "description": "<string>", // optional
    "parameters": { ... },     // optional (omit = empty params), JSON Schema object
    "strict": true             // optional, default false (Beta feature)
  }
}
```

Max 128 tools per request.

### `tool_choice` options

| Value | Behavior |
|-------|----------|
| `"none"` | model must NOT call tools (default when no tools in request) |
| `"auto"` | model may call tools or generate text (default when tools present) |
| `"required"` | model MUST call one or more tools |
| `{"type": "function", "function": {"name": "..."}}` | force a specific function |

### Streamed tool calls

The docs describe the OpenAI convention — tool calls arrive in `delta.tool_calls[]`:

| Field | Type | Notes |
|-------|------|-------|
| `index` | integer | disambiguates parallel tool calls across chunks |
| `id` | string | tool call ID, present in first chunk for this index |
| `type` | `"function"` | always "function" |
| `function.name` | string | function name, present in first chunk for this index |
| `function.arguments` | string | fragments concatenated across chunks (raw JSON string) |

The arguments are streamed as **concatenated string fragments** — the client accumulates them, then parses the final JSON. This matches OpenAI exactly.

**Parallel tool calls:** Supported. Multiple calls are disambiguated by `index` and `id` in the `tool_calls` array.

### Tool result message

```json
{
  "role": "tool",
  "tool_call_id": "<matches the call's id>",
  "content": "<string>"
}
```

- `content` is always a **string** — no structured/JSON content type documented.
- **There is no error channel.** Tool failures must be communicated via the content string itself (e.g. `"Error: file not found"`).

### `strict` mode (Beta)

When using `base_url="https://api.deepseek.com/beta"` and setting `"strict": true` on each tool function definition:
- Model output strictly conforms to the function's JSON schema.
- Server validates schemas upfront — rejects unsupported types.
- Supported JSON Schema types: `object`, `string`, `number`, `integer`, `boolean`, `array`, `enum`, `anyOf`, `$ref`/`$def`.
- Constraints: all `object` properties must be `required` with `additionalProperties: false`.
- Unsupported: `minLength`/`maxLength` for strings, `minItems`/`maxItems` for arrays.

### Decisions for `ur`
- `#[ur::tool]` should generate the standard `{type: "function", function: {...}}` shape.
- `Event::ToolCall { index, id, name, arguments }` where `arguments` is the raw accumulated JSON string — NOT parsed. Parsing is the caller's responsibility (the wire delivers raw JSON strings).
- `ToolResult.output: Result<String, String>` is a **local-only** convenience — the wire has no error channel. On send, turn `Err(msg)` into `content: msg` (or a structured error string).
- Serialize `tool_choice` as the appropriate string enum or named-tool object.
- `strict` mode should be an opt-in on tool definitions, gated behind the beta base URL.

---

## 6. Token usage & caching

### `usage` object field names (exact wire spelling)

```json
{
  "prompt_tokens": 16,                // = hit + miss
  "completion_tokens": 10,
  "total_tokens": 26,                 // = prompt + completion
  "prompt_cache_hit_tokens": 0,
  "prompt_cache_miss_tokens": 16,
  "completion_tokens_details": {
    "reasoning_tokens": 0
  }
}
```

Note: `prompt_tokens` is documented as `prompt_cache_hit_tokens + prompt_cache_miss_tokens`.

**Context caching:**
- Enabled **by default** for all users — no opt-in required.
- Cache hit/miss fields: `prompt_cache_hit_tokens` and `prompt_cache_miss_tokens`.
- Caching works on a "best-effort" basis, not guaranteed.
- Cache hit requires the new request's prefix to **fully match** a persisted cache prefix unit.
- Cache persistence happens at request boundaries, common prefix detection, and fixed token intervals.
- Caches auto-clear after hours to days of disuse.

**Reasoning token accounting:**
- `completion_tokens_details.reasoning_tokens` — tokens generated for reasoning/CoT.
- `completion_tokens` includes both reasoning and final answer tokens.

**Streaming usage:**
1. Set `stream_options: {"include_usage": true}` in the request.
2. The final SSE chunk carries `usage: <object>` and has `choices: []` (empty).
3. All prior chunks carry `usage: null`.
4. Without `stream_options.include_usage`, the final chunk still has `usage: null`.

### Decisions for `ur`
- `Usage` struct field names should match wire exactly:
  - `prompt_tokens`, `completion_tokens`, `total_tokens`
  - `prompt_cache_hit_tokens`, `prompt_cache_miss_tokens`
  - `completion_tokens_details.reasoning_tokens` → `reasoning_tokens: Option<u32>`
- Rename `cached_input_tokens` → `prompt_cache_hit_tokens` to match wire.
- Add `prompt_cache_miss_tokens` field.
- The `Event::Usage` variant should fire when the final chunk with usage arrives.
- Hardcode `stream_options: {"include_usage": true}` for all streaming requests.

---

## 7. Errors & limits

### HTTP status codes & error types

| Status | Label | Retry? | Note |
|--------|-------|--------|------|
| 400 | Invalid Format | No | Malformed request body |
| 401 | Authentication Fails | No | Wrong/missing API key |
| 402 | Insufficient Balance | No | Account out of funds |
| 422 | Invalid Parameters | No | Valid JSON but invalid params |
| 429 | Rate Limit Reached | Yes | Exceeded concurrency or RPM |
| 500 | Server Error | Yes | Transient server issue |
| 503 | Server Overloaded | Yes | High traffic |

The docs do NOT document the exact error body shape (no `error.message`/`error.type`/`error.code` shown), but since the API is OpenAI-compatible, the expected shape is:
```json
{"error": {"message": "...", "type": "...", "code": "..."}}
```
This should be verified with a live call (or could check the API reference more carefully — it shows full schemas for success responses but not error bodies).

### Rate limiting
- Signaled by HTTP 429.
- Concurrency limits: v4-pro=500, v4-flash=2500 (account-level).
- `user_id` isolation: each `user_id` sub-divides concurrency (same limits per user_id for expanded-quota accounts).
- No documented `Retry-After` header, but it should be expected per HTTP convention.
- No documented RPM/TPM limits — only concurrency.

### Retry strategy
- **Retryable:** 429 (with backoff), 500, 503.
- **Terminal:** 400, 401, 402, 422.
- The 10-minute pre-inference timeout is a connection timeout, not an error code.

### Decisions for `ur`
- `ur::Error` should have variants: `Auth`, `RateLimited`, `ServerError`, `BadRequest`, `InvalidParams`, `InsufficientFunds`, `Other(u16)`.
- Implement automatic retry with exponential backoff for 429, 500, 503.
- Include `Retry-After` header parsing for 429.

---

## 8. DeepSeek-specific gotchas

### reasoning_content lifecycle

This is the most important correctness trap:

1. **No tool calls between user messages:** The intermediate assistant's `reasoning_content` does NOT need to be passed back in subsequent requests. If passed, the API **ignores** it silently. See [Thinking Mode: Multi-turn Conversation](https://api-docs.deepseek.com/guides/thinking_mode#multi-turn-conversation).

2. **Tool calls between user messages:** The intermediate assistant's `reasoning_content` MUST be passed back in ALL subsequent requests (even subsequent user turns that follow later). If NOT passed, the API returns a **400 error**. See [Thinking Mode: Tool Calls](https://api-docs.deepseek.com/guides/thinking_mode#tool-calls).

3. **Streaming:** `reasoning_content` is accumulated the same way as `content` from chunks (`choices[0].delta.reasoning_content`). It must be stored separately and re-attached to the assistant message when building follow-up request messages.

4. **The assistant message object includes `reasoning_content`:** `response.choices[0].message` from non-streaming responses contains `content`, `reasoning_content`, and `tool_calls`. Appending the entire message object back to the messages list is sufficient — the server will read/write the `reasoning_content` field as appropriate.

### Beta-only features (require `/beta` base URL)
- **Chat Prefix Completion** — set `prefix: true` on the last assistant message. Only in chat completions. Optionally supply `reasoning_content` alongside the prefix.
- **FIM Completion** — uses `/completions` endpoint with `prompt` + `suffix`. Non-thinking mode only. Max output: 4K tokens.
- **strict-mode tool calls** — `"strict": true` on tool function definitions.

### Silent param drops & encoding quirks
- `temperature`, `top_p`, `presence_penalty`, `frequency_penalty` — all silently ignored in thinking mode (no error).
- `frequency_penalty` and `presence_penalty` are deprecated globally (not just thinking mode).
- `low`/`medium` reasoning effort → aliased to `high`. `xhigh` → aliased to `max`.
- JSON mode may occasionally return empty content (admitted known issue).
- `deepseek-chat` and `deepseek-reasoner` will be removed 2026-07-24.

### Other
- Multi-round conversation is fully stateless — the client must re-send the entire message history each request (standard OpenAI pattern).
- FIM has a hard 4K token output cap (way below the 384K chat cap).

### Decisions for `ur`
- `Session` must track whether any prior turn involved tool calls, and conditionally include/exclude `reasoning_content` from the re-sent assistant message. This is NOT optional — a 400 error results from getting it wrong.
- When in doubt, always include `reasoning_content` — it's never wrong to include it (ignored when unnecessary, required when tool calls happened).
- Chat Prefix Completion, FIM, and strict mode should be gated behind a beta base URL toggle.
- FIM endpoint (`/completions`) is a separate API call from chat completions — warrants its own client method or feature flag, and must disable thinking mode.
- Document that `temperature` and `top_p` are silently ignored in thinking mode.
- Emit deprecation warnings for `deepseek-chat`/`deepseek-reasoner` model IDs.

---

## Summary: decisions this forces in `ur`

| # | Decision |
|---|----------|
| 1 | Base URL: `"https://api.deepseek.com"` — no `/v1` prefix |
| 2 | Beta URL: `"https://api.deepseek.com/beta"` — separate base for Beta features |
| 3 | Default timeouts: generous (10+ min) to survive pre-inference wait |
| 4 | Model IDs: `deepseek-v4-flash`, `deepseek-v4-pro` — hardcode 1M context, 384K max output per model |
| 5 | Drop user-settable `context_window` — it's a model property |
| 6 | `Mode` field (thinking/non-thinking) gates which request params are sent vs. silently ignored |
| 7 | Never send `frequency_penalty` / `presence_penalty` (deprecated globally) |
| 8 | `stream_options: {"include_usage": true}` hardcoded for all streaming requests |
| 9 | SSE parser must handle `: keep-alive` comments |
| 10 | `Event::Done` carries `finish_reason` enum: `Stop`, `Length`, `ContentFilter`, `ToolCalls`, `InsufficientSystemResource` |
| 11 | `Usage` field names match wire: `prompt_tokens`, `completion_tokens`, `total_tokens`, `prompt_cache_hit_tokens`, `prompt_cache_miss_tokens`, `reasoning_tokens` |
| 12 | Rename `cached_input_tokens` → `prompt_cache_hit_tokens` |
| 13 | Add `prompt_cache_miss_tokens` to `Usage` |
| 14 | Tool call `arguments` arrive as concatenated string fragments — accumulate, parse externally |
| 15 | Tool results: `content` is always a string — `Result` mapping is local-only |
| 16 | `Session` must always include `reasoning_content` in assistant messages when re-sending (safe default) |
| 17 | Retry 429/500/503 with exponential backoff; fail fast on 400/401/402/422 |
| 18 | FIM uses separate `/completions` endpoint, requires non-thinking mode, caps output at 4K |
| 19 | Warn on deprecated model IDs (`deepseek-chat`, `deepseek-reasoner`) |
| 20 | `strict` tool mode is Beta-only, requires beta base URL |
