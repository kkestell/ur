# `ur-openai` — OpenAI provider

`ur-openai` is a [`Provider`](API.md#8-provider-seam) implementation for OpenAI Chat Completions. It is reached as `ur::openai` when the `openai` feature is enabled on the `ur` facade.

This document covers the OpenAI-specific client, generation-setting mapping, tool behavior, retry/timeout behavior, and wire mapping. The provider-agnostic agent/tool/session surface is specified in [API.md](API.md).

- Built on `reqwest`, so it requires a Tokio reactor at run time.
- Implements `Provider::chat` with streaming Chat Completions.
- `Provider::model_spec` returns `None` for v1 because OpenAI model ids and limits change frequently.

## Installation

The `openai` feature is on by default:

```toml
[dependencies]
ur = "0.1"

# or: DeepSeek instead of the default OpenAI provider
ur = { version = "0.1", default-features = false, features = ["serde", "deepseek"] }
```

On the facade it is an optional-dependency feature in `default`: `ur-openai = { optional = true }`, `openai = ["dep:ur-openai"]`, and `default = ["serde", "openai"]`.

## 1. Client — `OpenAiClient`

A cheap-to-clone handle (`Arc` inside) over an HTTP connection pool, auth, and retry policy.

```rust
impl OpenAiClient {
    /// Reads the key from `$OPENAI_API_KEY`.
    pub fn try_from_env() -> Result<Self>;

    /// Reads the key from `$OPENAI_API_KEY`. Panics if unset.
    pub fn from_env() -> Self;

    /// A client with the given API key and otherwise-default settings.
    pub fn new(api_key: impl Into<String>) -> Self;

    pub fn builder() -> OpenAiClientBuilder;
}
```

### `OpenAiClientBuilder`

Non-consuming builder:

```rust
impl OpenAiClientBuilder {
    pub fn api_key(&mut self, key: impl Into<String>) -> &mut Self;
    pub fn base_url(&mut self, url: impl Into<String>) -> &mut Self;
    pub fn user(&mut self, user: impl Into<String>) -> &mut Self;
    pub fn timeout(&mut self, dur: std::time::Duration) -> &mut Self;
    pub fn max_retries(&mut self, n: u32) -> &mut Self;
    pub fn http_client(&mut self, client: OpenAiHttpClient) -> &mut Self;
    pub fn build(&mut self) -> Result<OpenAiClient>;
}
```

Default base URL is `https://api.openai.com/v1`. `user` is optional and is sent as OpenAI's `user` field. It must match `[a-zA-Z0-9_-]{1,512}`.

```rust
#[derive(Clone, Debug)]
pub struct OpenAiHttpClient { /* private */ }

impl OpenAiHttpClient {
    pub fn from_reqwest(client: reqwest::Client) -> Self;
}
```

## 2. Generation settings

`Model<OpenAiClient>` exposes the provider-agnostic settings from [API.md §3](API.md#3-model--modelp). OpenAI Chat Completions maps them as follows:

- `max_tokens` is sent as `max_completion_tokens`.
- `temperature` must be in `0.0..=2.0`.
- `top_p` must be in `0.0..=1.0`.
- `stop` accepts at most 4 entries.
- `response_format = JsonObject` sends `{ "type": "json_object" }`.
- `ReasoningEffort::Low`, `Medium`, and `High` map directly. `ExtraHigh` and `Max` map to `high`.
- `Thinking` is ignored because Chat Completions has no matching request field.

Invalid settings surface as `Error::Config` before any HTTP request is sent.

## 3. Tools and strict mode

Tools are sent as OpenAI function tools. A strict `ToolSchema` is rewritten into the constrained subset: objects are closed with `additionalProperties: false`, every property is listed in `required`, originally-optional properties become nullable, and unsupported size keywords are dropped.

Unlike DeepSeek, OpenAI accepts mixed strict and non-strict tools, so each tool's `strict` flag is encoded independently.

## 4. Retries, timeouts, errors

The provider retries HTTP `408`, `409`, `429`, `500`, `502`, `503`, and `504` with exponential backoff, honoring numeric `Retry-After` on rate limits, up to `max_retries` (default 3). Request timeouts and connection-establishment failures are retried as transport failures.

Status mapping:

| Status | `Error`                       | Retry |
| ------ | ----------------------------- | ----- |
| 400    | `BadRequest`                  | no    |
| 401    | `Auth`                        | no    |
| 402    | `InsufficientFunds`           | no    |
| 403    | `Auth`                        | no    |
| 404    | `InvalidParams`               | no    |
| 408    | `Server { 408, .. }`          | yes   |
| 409    | `Server { 409, .. }`          | yes   |
| 422    | `InvalidParams`               | no    |
| 429    | `RateLimited { retry_after }` | yes   |
| 500    | `Server { 500, .. }`          | yes   |
| 502    | `Server { 502, .. }`          | yes   |
| 503    | `Server { 503, .. }`          | yes   |
| 504    | `Server { 504, .. }`          | yes   |
| other  | `Server { status, .. }`       | no    |

## 5. Wire mapping

Endpoint: `POST {base_url}/chat/completions`. Header: `Authorization: Bearer {api_key}`.

Request body:

```json
{
  "model": "gpt-5.5",
  "messages": [],
  "stream": true,
  "stream_options": { "include_usage": true },
  "max_completion_tokens": 4096,
  "reasoning_effort": "high",
  "response_format": { "type": "json_object" },
  "tools": [],
  "tool_choice": "auto",
  "user": "tenant-1"
}
```

Message shapes:

```json
{"role": "system", "content": "<text>"}
{"role": "user", "content": "<text>"}
{"role": "assistant", "content": "<text|null>",
 "tool_calls": [{"id": "...", "type": "function",
                 "function": {"name": "...", "arguments": "<json string>"}}]}
{"role": "tool", "tool_call_id": "<id>", "content": "<string>"}
```

Streaming chunk mapping:

| Chunk field                           | `RawEvent`                                           |
| ------------------------------------- | ---------------------------------------------------- |
| `choices[0].delta.content`            | `TextDelta(content)`                                 |
| `choices[0].delta.tool_calls[i]`      | `ToolCallDelta { index, id, name, arguments }`       |
| deprecated `choices[0].delta.function_call` | `ToolCallDelta` at index `0` with a synthetic id       |
| `choices[0].finish_reason`            | terminal reason recorded for `Done`                  |
| final chunk `usage` with empty choices | `Usage` carried on `Done`                            |
| `data: [DONE]`                        | end of provider stream                               |

Finish reasons map as: `stop` -> `Stop`, `length` -> `Length`, `content_filter` -> `ContentFilter`, `tool_calls` and deprecated `function_call` -> `ToolCalls`, unknown strings -> `Other`. The deprecated `delta.function_call` stream shape is accepted only as a compatibility path; new requests use modern `tools`.

## 6. Complete example

```rust
use futures_util::StreamExt;

#[ur::tool(description = "Add two integers.")]
async fn add(a: i64, b: i64) -> i64 { a + b }

#[tokio::main]
async fn main() -> ur::Result<()> {
    let client = ur::openai::OpenAiClient::try_from_env()?;
    let model = ur::Model::new(client, "gpt-5.5").max_tokens(128);
    let agent = ur::Agent::new("You are concise. Use tools when useful.", model).tool(add);
    let mut session = agent.session();

    let mut events = session.send("What is 41 + 1? Use the tool.");
    while let Some(event) = events.next().await {
        match event? {
            ur::Event::TextDelta { delta } => print!("{delta}"),
            ur::Event::Done { .. } => break,
            _ => {}
        }
    }
    Ok(())
}
```
