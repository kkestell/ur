# `ur-deepseek` — DeepSeek provider

`ur-deepseek` is a [`Provider`](API.md#8-provider-seam) implementation for DeepSeek, plus a compiled-in model catalog. It is reached as `ur::deepseek` when the `deepseek` feature is enabled on the `ur` facade.

This document covers everything DeepSeek-specific: the client and its builder, how DeepSeek interprets the provider-agnostic generation settings, the reasoning-content lifecycle, strict mode, retry/timeout behavior, and the exact wire mapping. The provider-agnostic agent/tool/session surface and the `Provider` seam this crate implements are specified in [API.md](API.md).

- Built on `reqwest`, so it requires a Tokio reactor at run time.
- Implements `Provider::chat` (streaming-only internally), `Provider::model_spec` (the catalog below), and `Provider::model_notice` (deprecated-id notices).

## Installation

The `deepseek` feature is optional. OpenAI is the default provider, so enable DeepSeek explicitly:

```toml
[dependencies]
# default: OpenAI included
ur = "0.1"
# DeepSeek instead
ur = { version = "0.1", default-features = false, features = ["serde", "deepseek"] }
```

On the facade it is an optional-dependency feature: `ur-deepseek = { optional = true }` and `deepseek = ["dep:ur-deepseek"]`.

---

## 1. Client — `DeepSeekClient`

A cheap-to-clone handle (`Arc` inside) over an HTTP connection pool, auth, and retry policy. It is a `Provider`; it is the thing you hand to a `Model`.

```rust
impl DeepSeekClient {
    /// Reads the key from `$DEEPSEEK_API_KEY`.
    pub fn try_from_env() -> Result<Self>;

    /// Reads the key from `$DEEPSEEK_API_KEY`. Panics if unset.
    pub fn from_env() -> Self;

    /// A client with the given API key and otherwise-default settings.
    /// Infallible: builder validation applies only to overrides, and the
    /// defaults are valid.
    pub fn new(api_key: impl Into<String>) -> Self;

    pub fn builder() -> DeepSeekClientBuilder;
}
```

`from_env()` is a convenience wrapper around `try_from_env().expect(...)`; fallible code should prefer `try_from_env()`.

### `DeepSeekClientBuilder`

Non-consuming builder (C-BUILDER); every method takes `&mut self` and returns `&mut Self`, so both one-liners and conditional configuration work.

```rust
impl DeepSeekClientBuilder {
    /// API key. If never set, falls back to `$DEEPSEEK_API_KEY` at build time.
    pub fn api_key(&mut self, key: impl Into<String>) -> &mut Self;

    /// Override the base URL. Default: "https://api.deepseek.com" (no `/v1`).
    pub fn base_url(&mut self, url: impl Into<String>) -> &mut Self;

    /// Use the beta base URL ("https://api.deepseek.com/beta"), required for
    /// strict-mode tools and prefix completion. Default: false.
    pub fn beta(&mut self, enabled: bool) -> &mut Self;

    /// Optional content-safety / cache / scheduling isolation key.
    /// Must match `[a-zA-Z0-9\-_]+`, max 512 chars. Sent as `user_id` in the body.
    pub fn user_id(&mut self, id: impl Into<String>) -> &mut Self;

    /// Per-request timeout. Default: 15 minutes (the server may hold a connection
    /// up to 10 minutes before inference starts).
    pub fn timeout(&mut self, dur: std::time::Duration) -> &mut Self;

    /// Max automatic retries for retryable HTTP statuses and transient transport
    /// failures (see §5). Default: 3.
    pub fn max_retries(&mut self, n: u32) -> &mut Self;

    /// Supply a preconfigured HTTP client (proxies, custom TLS, etc.).
    pub fn http_client(&mut self, client: DeepSeekHttpClient) -> &mut Self;

    /// Validate the configuration and construct the client. Returns
    /// `Err(Error::Config { .. })` if no API key is available (neither set here
    /// nor in `$DEEPSEEK_API_KEY`), if `base_url` is not a valid URL, or if
    /// `user_id` does not match `[a-zA-Z0-9\-_]{1,512}`.
    pub fn build(&mut self) -> Result<DeepSeekClient>;
}
```

```rust
#[derive(Clone, Debug)]
pub struct DeepSeekHttpClient { /* private */ }

impl DeepSeekHttpClient {
    /// Wrap a preconfigured reqwest client for the DeepSeek provider.
    pub fn from_reqwest(client: reqwest::Client) -> Self;
}
```

`base_url` and `beta` interact: setting `beta(true)` selects the beta host unless `base_url` was set explicitly, in which case the explicit URL wins.

Stability note: `DeepSeekHttpClient::from_reqwest` is the one constructor that names `reqwest` in a public signature. Before a 1.0 release, either `reqwest` must be 1.x or that constructor must move behind a compatibility boundary.

---

## 2. Generation settings under DeepSeek

`Model<DeepSeekClient>` exposes the provider-agnostic settings ([API.md §3](API.md#3-model--modelp)). DeepSeek interprets and validates them as follows:

- **`temperature` / `top_p` are silently ignored by the backend when thinking is on.** `ur` therefore sends them only when `Thinking::Disabled`; otherwise it omits them so the request reflects reality.
- **`reasoning_effort` aliasing.** `Low`/`Medium` are accepted but aliased to `High`; `ExtraHigh` aliases to `Max`. The full `ReasoningEffort` set is kept so callers' intent survives the round trip.
- **Local range checks.** As the request is assembled, the provider validates `temperature` (`0.0..=2.0`), `top_p` (`0.0..=1.0`), and `stop` (≤ 16 entries); `max_tokens` is validated as ≥ 1 and, for catalogued models, ≤ `max_output`. An out-of-range value surfaces as `Error::Config` on the `send` stream before any network round-trip.
- **`frequency_penalty` / `presence_penalty`** are not exposed by `ur` — deprecated backend-wide with no effect.
- **Deprecated model ids.** `Provider::model_notice` returns `ModelNotice::Deprecated` for `deepseek-chat` and `deepseek-reasoner` (removed 2026-07-24). `Model::new` resolves that notice once and emits one `tracing` warning for each constructed model. `Provider::model_spec` remains a pure catalog lookup and does not log.

Unlike `FinishReason`, `ReasoningEffort` has no open-ended variant; DeepSeek consumes the standard set above.

---

## 3. Reasoning-content lifecycle (correctness-critical)

The backend's rule for thinking models:

- If a turn involved **no** tool calls, echoing its `reasoning_content` back is harmless (the server ignores it).
- If a turn **did** involve tool calls, its `reasoning_content` **must** be present in every subsequent request or the server returns **400**.

`Session` already retains each assistant turn's `reasoning_content` and replays the full history on every turn ([API.md §8](API.md#8-provider-seam)). The DeepSeek provider serializes that `reasoning_content` into each assistant message (see the message shapes in §6), so the always-safe path is taken automatically: callers do nothing, and the 400 cannot occur by construction.

---

## 4. Strict mode

`ToolSchema::strict(true)` requires the beta base URL (`beta(true)`) and applies to the whole request: DeepSeek requires every function in `tools` to carry `strict: true`, so if any registered tool is strict, `ur` marks them all strict. Mixing strict and non-strict tools, or setting strict without the beta URL, is rejected locally as `Error::Config`.

For each strict tool the provider rewrites the macro-generated schema into DeepSeek's strict subset: objects are closed (`additionalProperties: false`) with every property listed in `required`, and an `Option<T>` parameter becomes required-but-nullable (`"type": ["…", "null"]`). Keywords strict mode does not support (`minLength`/`maxLength`, `minItems`/`maxItems`) are dropped.

---

## 5. Retries, timeouts, keep-alive

- The provider retries HTTP `408`, `429`, `500`, `502`, `503`, and `504` with exponential backoff (honoring `Retry-After` on 429) up to `max_retries` (default 3). Request timeouts and connection-establishment failures are retried as `Transport` errors; TLS/certificate failures, decode failures, and HTTP `400`, `401`, `402`, and `422` fail immediately.
- Default per-request timeout is 15 minutes; the server may hold a connection up to 10 minutes before inference starts.
- The SSE parser ignores `: keep-alive` comment lines and blank lines, and treats `data: [DONE]` as end-of-stream.

---

## 6. Wire mapping

This section is the implementation reference for the `deepseek` module. Endpoint: `POST {base_url}/chat/completions`. Header: `Authorization: Bearer {api_key}`.

### Model catalog (compiled-in)

`Provider::model_spec(id)` returns `Some(ModelSpec { context_window, max_output })` for these, `None` otherwise. `Provider::model_notice(id)` returns a deprecation notice only for the deprecated ids in the table.

| Model id                              | context_window | max_output |
| ------------------------------------- | -------------: | ---------: |
| `deepseek-v4-flash`                   |      1_000_000 |    384_000 |
| `deepseek-v4-pro`                     |      1_000_000 |    384_000 |
| `deepseek-chat` (dep. 2026-07-24)     |      1_000_000 |    384_000 |
| `deepseek-reasoner` (dep. 2026-07-24) |      1_000_000 |    384_000 |

### Request body

Built from history + tool schemas + `Model` settings:

```json
{
  "model": "deepseek-v4-pro",
  "messages": [ /* see message shapes below */ ],
  "stream": true,
  "stream_options": { "include_usage": true },

  "thinking": {"type": "enabled"},          // omit when Thinking::Default
  "reasoning_effort": "high",               // omit unless set
  "max_tokens": 4096,                        // omit unless set; cap if model is catalogued
  "stop": ["\n\n"],                          // omit unless set; ≤ 16
  "response_format": {"type": "json_object"},// omit for Text
  "temperature": 0.7,                        // ONLY when Thinking::Disabled
  "top_p": 0.9,                              // ONLY when Thinking::Disabled
  "tools": [ /* tool schemas */ ],           // omit when none
  "tool_choice": "auto",                     // omit when no tools
  "user_id": "..."                           // omit unless set on the client
}
```

`stream` and `stream_options.include_usage` are always set — `ur` is streaming-only internally.

Message shapes:

```json
{"role": "system",    "content": "<text>"}
{"role": "user",      "content": "<text>"}
{"role": "assistant", "content": "<text|null>", "reasoning_content": "<text|null>",
                      "tool_calls": [ {"id": "...", "type": "function",
                                       "function": {"name": "...", "arguments": "<json string>"}} ]}
{"role": "tool",      "tool_call_id": "<id>", "content": "<string>"}
```

Tool schema (per registered tool):

```json
{"type": "function",
 "function": {"name": "...", "description": "...", "parameters": { /* JSON Schema */ },
              "strict": false}}
```

See §4 for how `strict` is normalized across the tool set.

### Streaming chunk → `RawEvent`

Each `data:` line is one JSON object:

```json
{"id":"...","object":"chat.completion.chunk","created":1718345013,"model":"...",
 "choices":[{"index":0,
   "delta":{"role":"assistant","content":"Hi","reasoning_content":null,"tool_calls":null},
   "finish_reason":null}],
 "usage":null}
```

Mapping:

| Chunk field                                     | `RawEvent`                                                                                         |
| ----------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `choices[0].delta.content` (non-null)           | `TextDelta(content)`                                                                               |
| `choices[0].delta.reasoning_content` (non-null) | `ReasoningDelta(reasoning_content)`                                                                |
| `choices[0].delta.tool_calls[i]`                | `ToolCallDelta { index, id, name, arguments }` (id/name in first fragment; arguments concatenated) |
| `choices[0].finish_reason` (non-null)           | record reason for `Done`                                                                           |
| final chunk `usage` (non-null, `choices: []`)   | `Usage` carried on `Done`                                                                          |
| `data: [DONE]`                                  | end of provider stream                                                                             |

`finish_reason` strings map to `FinishReason`: `stop`→`Stop`, `length`→`Length`, `content_filter`→`ContentFilter`, `tool_calls`→`ToolCalls`, `insufficient_system_resource`→`Other("insufficient_system_resource")`.

`usage` JSON → `Usage`: `prompt_tokens`, `completion_tokens`, `total_tokens` map directly; `prompt_cache_hit_tokens` → `cached_prompt_tokens` (DeepSeek also reports `prompt_cache_miss_tokens`, where `prompt_tokens == prompt_cache_hit_tokens + prompt_cache_miss_tokens`); `completion_tokens_details.reasoning_tokens` → `reasoning_tokens`.

### Status → `Error`

| Status | `Error`                       | Retry |
| ------ | ----------------------------- | ----- |
| 400    | `BadRequest`                  | no    |
| 401    | `Auth`                        | no    |
| 402    | `InsufficientFunds`           | no    |
| 408    | `Server { 408, .. }`          | yes   |
| 422    | `InvalidParams`               | no    |
| 429    | `RateLimited { retry_after }` | yes   |
| 500    | `Server { 500, .. }`          | yes   |
| 502    | `Server { 502, .. }`          | yes   |
| 503    | `Server { 503, .. }`          | yes   |
| 504    | `Server { 504, .. }`          | yes   |
| other  | `Server { status, .. }`       | no    |

Error body is the OpenAI shape `{"error": {"message", "type", "code"}}`. For the variants that carry a `message` field — `BadRequest`, `InvalidParams`, `Server` — the body message populates it. `Auth` (401) and `InsufficientFunds` (402) are parameterless: the status alone determines the condition and the (generic) body message is not retained.

---

## 7. Trait impls

- `DeepSeekClient` implements `Debug` via a manual, opaque, struct-named summary; it is always `Clone` and cheap to clone.
- `DeepSeekHttpClient` derives `Clone` and `Debug`.

---

## 8. Complete example

```rust
use futures_util::StreamExt;
use serde::Serialize;

#[ur::tool(description = "Add two integers.")]
async fn add(a: i64, b: i64) -> i64 { a + b }

#[derive(Serialize)]
struct Weather { temp_c: f64, summary: String }

#[ur::tool(description = "Look up the current weather for a city.")]
async fn weather(city: String) -> Result<Weather, std::io::Error> {
    Ok(Weather { temp_c: 18.5, summary: format!("clear skies over {city}") })
}

#[tokio::main]
async fn main() -> ur::Result<()> {
    let client = ur::deepseek::DeepSeekClient::try_from_env()?;
    let model = ur::Model::new(client, "deepseek-v4-pro");

    let agent = ur::Agent::new("You are a concise assistant. Use tools when useful.", model)
        .tool(add)
        .tool(weather);

    let mut session = agent.session();
    let mut events = session.send("What is 41 + 1? Use the tool.");
    while let Some(event) = events.next().await {
        match event? {
            ur::Event::TextDelta { delta } => print!("{delta}"),
            ur::Event::ReasoningDelta { .. } => {}
            ur::Event::ToolCall { name, arguments, .. } => eprintln!("\ncall {name}({arguments})"),
            ur::Event::ToolResult { output, .. } => match output {
                ur::ToolOutput::Ok(v) => eprintln!("result: {v}"),
                ur::ToolOutput::Err(e) => eprintln!("error: {e}"),
            },
            ur::Event::Usage { usage } => eprintln!(
                "tokens: in={} (cached {}) out={}",
                usage.prompt_tokens,
                usage.cached_prompt_tokens.unwrap_or(0),
                usage.completion_tokens,
            ),
            ur::Event::Done { .. } => break,
            _ => {}
        }
    }
    Ok(())
}
```
