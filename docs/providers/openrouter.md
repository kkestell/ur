# OpenRouter

## Installation
The optional `openrouter` feature and facade wiring.

## Client
`OpenRouterClient` as a cheap-to-clone handle and `Provider`.

### Constructing a client
`try_from_env` / `from_env` / `new` and the `$OPENROUTER_API_KEY` fallback.

### `OpenRouterClientBuilder`
Knobs: `api_key`, `base_url`, `user`, `referer`, `title`, `provider_routing`, `timeout`, `max_retries`, `http_client`.

### App attribution
Optional `HTTP-Referer` / `X-Title` headers for leaderboard attribution.

### `ProviderRouting`
The client-level `provider` routing object (order, allow_fallbacks, sort, only, ignore).

## Models
Namespaced ids (`openai/gpt-5.5`) passed verbatim, with unknown-id rejection.

## Generation setting mapping
OpenRouter-specific mapping including `max_completion_tokens` and ranges.

### The `reasoning` object
`Thinking` and `ReasoningEffort` merged into one `reasoning` object (`enabled` + `effort`, with `xhigh`).

## Tools and strict mode
Function tools with independent per-tool `strict` flags and the shared schema rewriter.

## Retries, timeouts, errors
Retryable statuses and the status → `Error` table with OpenRouter-specific meanings.

## Wire mapping
Endpoint, headers, request body (with optional `provider` object), and message shapes.

### Streaming chunks → `RawEvent`
The chunk-field → event mapping, including `delta.reasoning`, and `: OPENROUTER PROCESSING` keep-alive handling.

## Examples
The OpenRouter example targets (`openrouter`, `structured_openrouter`) and prerequisites.
