# `ur-openrouter` — OpenRouter provider

`ur-openrouter` is a [`Provider`](API.md#8-provider-seam) implementation for [OpenRouter](https://openrouter.ai), an OpenAI-compatible aggregator that fronts models from many upstream providers behind one API. It is reached as `ur::openrouter` when the `openrouter` feature is enabled on the `ur` facade.

This document covers the OpenRouter-specific client, app-attribution headers, provider routing, generation-setting mapping, tool behavior, retry/timeout behavior, and wire mapping. The provider-agnostic agent/tool/session surface is specified in [API.md](API.md).

- Built on `reqwest`, so it requires a Tokio reactor at run time.
- Implements `Provider::chat` with streaming Chat Completions.
- `Provider::model_spec` returns `None`: OpenRouter exposes hundreds of models across providers and the list is dynamic, so model ids pass through untouched.

## Installation

```toml
[dependencies]
ur = { version = "0.1", default-features = false, features = ["serde", "openrouter"] }
```

On the facade it is an optional-dependency feature: `ur-openrouter = { optional = true }`, `openrouter = ["dep:ur-openrouter"]`. It is not part of `default`.

## 1. Client — `OpenRouterClient`

A cheap-to-clone handle (`Arc` inside) over an HTTP connection pool, auth, and retry policy.

```rust
impl OpenRouterClient {
    /// Reads the key from `$OPENROUTER_API_KEY`.
    pub fn try_from_env() -> Result<Self>;

    /// Reads the key from `$OPENROUTER_API_KEY`. Panics if unset.
    pub fn from_env() -> Self;

    /// A client with the given API key and otherwise-default settings.
    pub fn new(api_key: impl Into<String>) -> Self;

    pub fn builder() -> OpenRouterClientBuilder;
}
```

### `OpenRouterClientBuilder`

Non-consuming builder:

```rust
impl OpenRouterClientBuilder {
    pub fn api_key(&mut self, key: impl Into<String>) -> &mut Self;
    pub fn base_url(&mut self, url: impl Into<String>) -> &mut Self;
    pub fn user(&mut self, user: impl Into<String>) -> &mut Self;
    pub fn referer(&mut self, referer: impl Into<String>) -> &mut Self;
    pub fn title(&mut self, title: impl Into<String>) -> &mut Self;
    pub fn provider_routing(&mut self, routing: ProviderRouting) -> &mut Self;
    pub fn timeout(&mut self, dur: std::time::Duration) -> &mut Self;
    pub fn max_retries(&mut self, n: u32) -> &mut Self;
    pub fn http_client(&mut self, client: OpenRouterHttpClient) -> &mut Self;
    pub fn build(&mut self) -> Result<OpenRouterClient>;
}
```

Default base URL is `https://openrouter.ai/api/v1`. `user` is optional and is sent as the `user` field; it must match `[a-zA-Z0-9_-]{1,512}`.

### App attribution

`referer` and `title` set the optional `HTTP-Referer` and `X-Title` headers. OpenRouter uses them to attribute traffic to your app on its public leaderboard. Both are off by default and only sent when configured.

### Provider routing

`ProviderRouting` is a client-level preference applied to every request, serialized into OpenRouter's `provider` object. Leave fields at their defaults to use OpenRouter's own routing.

```rust
pub struct ProviderRouting {
    pub order: Vec<String>,            // provider slugs to try, in order
    pub allow_fallbacks: Option<bool>, // allow providers outside order/only
    pub sort: Option<String>,          // "price" | "throughput" | "latency"
    pub only: Vec<String>,             // restrict to these providers
    pub ignore: Vec<String>,           // never route to these providers
}
```

An all-default `ProviderRouting` is dropped at build time, so no `provider` object is sent.

## 2. Models

OpenRouter model ids are namespaced by upstream provider, e.g. `openai/gpt-5.5`, `deepseek/deepseek-chat`. Pass the id straight to `Model::new`; it is sent verbatim. Unknown ids are rejected by OpenRouter as a `400`/`404`.

## 3. Generation settings

`Model<OpenRouterClient>` exposes the provider-agnostic settings from [API.md §3](API.md#3-model--modelp). OpenRouter maps them as follows:

- `max_tokens` is sent as `max_completion_tokens`.
- `temperature` must be in `0.0..=2.0`.
- `top_p` must be in `0.0..=1.0`.
- `stop` accepts at most 4 entries.
- `response_format = JsonObject` sends `{ "type": "json_object" }`.
- `response_format = JsonSchema(..)` sends `{ "type": "json_schema", "json_schema": { "name", "schema", "strict", optional "description" } }`. A strict schema (the default) is rewritten into the constrained subset described in §4 before it is sent. The schema name must match `[A-Za-z0-9_-]{1,64}`.
- `Thinking` and `ReasoningEffort` are merged into a single `reasoning` object (unlike OpenAI's flat `reasoning_effort`):
  - `Thinking::Enabled` → `reasoning: { "enabled": true }`; `Thinking::Disabled` → `reasoning: { "enabled": false }`; `Thinking::Default` adds nothing.
  - `ReasoningEffort::Low`/`Medium`/`High` map directly to `reasoning.effort`. `ExtraHigh` and `Max` map to `xhigh` (OpenRouter's top effort).
  - When neither is set, no `reasoning` object is sent.

Invalid settings surface as `Error::Config` before any HTTP request is sent.

## 4. Tools and strict mode

Tools are sent as OpenAI-style function tools. A strict `ToolSchema` is rewritten into the constrained subset: objects are closed with `additionalProperties: false`, every property is listed in `required`, originally-optional properties become nullable, and unsupported size keywords are dropped. Each tool's `strict` flag is encoded independently, so strict and non-strict tools can be mixed.

## 5. Retries, timeouts, errors

The provider retries HTTP `408`, `409`, `429`, `500`, `502`, `503`, and `504` with exponential backoff, honoring numeric `Retry-After` on rate limits, up to `max_retries` (default 3). Request timeouts and connection-establishment failures are retried as transport failures. Error messages are read from OpenRouter's `{ "error": { "code", "message", "metadata" } }` body.

Status mapping:

| Status | `Error`                       | Retry | OpenRouter meaning                             |
| ------ | ----------------------------- | ----- | ---------------------------------------------- |
| 400    | `BadRequest`                  | no    | invalid or missing params                      |
| 401    | `Auth`                        | no    | invalid credentials                            |
| 402    | `InsufficientFunds`           | no    | insufficient credits                           |
| 403    | `Auth`                        | no    | moderation/guardrail block (reason in message) |
| 404    | `InvalidParams`               | no    | unknown model/route                            |
| 408    | `Server { 408, .. }`          | yes   | request timed out                              |
| 409    | `Server { 409, .. }`          | yes   | conflict                                       |
| 422    | `InvalidParams`               | no    | unprocessable params                           |
| 429    | `RateLimited { retry_after }` | yes   | rate limited                                   |
| 500    | `Server { 500, .. }`          | yes   | server error                                   |
| 502    | `Server { 502, .. }`          | yes   | upstream model down / invalid response         |
| 503    | `Server { 503, .. }`          | yes   | no provider meets routing requirements         |
| 504    | `Server { 504, .. }`          | yes   | gateway timeout                                |
| other  | `Server { status, .. }`       | no    |                                                |

## 6. Wire mapping

Endpoint: `POST {base_url}/chat/completions`. Header: `Authorization: Bearer {api_key}`, plus optional `HTTP-Referer` and `X-Title`.

Request body:

```json
{
  "model": "openai/gpt-5.5",
  "messages": [],
  "stream": true,
  "stream_options": { "include_usage": true },
  "max_completion_tokens": 4096,
  "reasoning": { "enabled": true, "effort": "high" },
  "response_format": { "type": "json_object" },
  "tools": [],
  "tool_choice": "auto",
  "provider": { "order": ["openai"], "allow_fallbacks": false },
  "user": "tenant-1"
}
```

Message shapes match OpenAI Chat Completions:

```json
{"role": "system", "content": "<text>"}
{"role": "user", "content": "<text>"}
{"role": "assistant", "content": "<text|null>",
 "tool_calls": [{"id": "...", "type": "function",
                 "function": {"name": "...", "arguments": "<json string>"}}]}
{"role": "tool", "tool_call_id": "<id>", "content": "<string>"}
```

Streaming chunk mapping:

| Chunk field                            | `RawEvent`                                     |
| -------------------------------------- | ---------------------------------------------- |
| `choices[0].delta.reasoning`           | `ReasoningDelta(reasoning)`                    |
| `choices[0].delta.content`             | `TextDelta(content)`                           |
| `choices[0].delta.tool_calls[i]`       | `ToolCallDelta { index, id, name, arguments }` |
| `choices[0].finish_reason`             | terminal reason recorded for `Done`            |
| final chunk `usage` with empty choices | `Usage` carried on `Done`                      |
| `data: [DONE]`                         | end of provider stream                         |

OpenRouter interleaves `: OPENROUTER PROCESSING` keep-alive comment lines into the stream; these are ignored like any SSE comment. Finish reasons map as: `stop` -> `Stop`, `length` -> `Length`, `content_filter` -> `ContentFilter`, `tool_calls` -> `ToolCalls`, unknown strings -> `Other`.

## 7. Complete example

Example targets require the `openrouter` feature and `$OPENROUTER_API_KEY`:

- `openrouter` — the full tool-using flow below.
- `structured_openrouter` — `ResponseFormat::json_schema_for::<T>` output parsed back into a Rust type (§3).

`crates/ur/examples/openrouter.rs`:

```rust
use futures_util::StreamExt;

#[ur::tool(description = "Add two integers.")]
async fn add(a: i64, b: i64) -> i64 { a + b }

#[tokio::main]
async fn main() -> ur::Result<()> {
    let client = ur::openrouter::OpenRouterClient::builder()
        .referer("https://github.com/kkestell/ur")
        .title("ur example")
        .build()?;
    let model = ur::Model::new(client, "deepseek/deepseek-v4-flash").max_tokens(128);
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
