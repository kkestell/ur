# OpenAI

## Installation
The `openai` default feature and facade wiring.

## Client
`OpenAiClient` as a cheap-to-clone handle over auth, connection pool, and retry policy.

### Constructing a client
`try_from_env` / `from_env` / `new` and their environment-key fallback.

### `OpenAiClientBuilder`
The non-consuming builder knobs: `api_key`, `base_url`, `user`, `timeout`, `max_retries`, `http_client`.

### Base URL overrides
Pointing `base_url` at any OpenAI-compatible endpoint.

### `OpenAiHttpClient`
Wrapping a preconfigured `reqwest` client.

## Generation setting mapping
How each provider-agnostic setting maps onto Chat Completions (`max_completion_tokens`, ranges, response formats).

### `ReasoningEffort` mapping
`Low`/`Medium`/`High` direct; `ExtraHigh`/`Max` collapse to `high`.

### `Thinking`
Ignored because Chat Completions has no matching field.

## Tools and strict mode
Function tools, independent per-tool `strict` flags, and the shared schema rewriter.

## Retries, timeouts, errors
Retryable statuses, backoff/`Retry-After` handling, and the status → `Error` table.

## Wire mapping
Endpoint, auth header, request-body shape, and message shapes.

### Request body
The assembled JSON body with `stream`/`stream_options` and conditional fields.

### Message shapes
System/user/assistant/tool JSON the provider emits.

### Streaming chunks → `RawEvent`
The chunk-field → event mapping and `finish_reason` translation.

## Examples
The OpenAI example targets and what each demonstrates.
