# DeepSeek

## Installation
The optional `deepseek` feature and facade wiring.

## Client
`DeepSeekClient` as a cheap-to-clone handle and `Provider`.

### Constructing a client
`try_from_env` / `from_env` / `new` and the `$DEEPSEEK_API_KEY` fallback.

### `DeepSeekClientBuilder`
Knobs: `api_key`, `base_url`, `beta`, `user_id`, `timeout`, `max_retries`, `http_client`.

### Beta mode
`beta(true)` selects the beta base URL required for strict tools and prefix completion.

## Generation setting mapping
DeepSeek-specific interpretation and validation of each provider-agnostic setting.

### Thinking-gated sampling
`temperature`/`top_p` are omitted when thinking is on; aliases and range checks.

### `ReasoningEffort` aliasing
`Low`/`Medium`→`High`, `ExtraHigh`→`Max`, full set preserved.

### `response_format`
`JsonObject` accepted; `JsonSchema` rejected as `Config` (see structured-outputs).

## Reasoning-content lifecycle
The correctness rule that reasoning must replay with tool-call turns and how `Session` handles it automatically.

## Strict mode
All-or-nothing strict tools requiring the beta URL, and how `ur` enforces uniformity.

## Retries, timeouts, errors
Retryable statuses, the 15-minute default timeout, keep-alive handling, and the status → `Error` table.

## Model catalog
The compiled-in model ids with `context_window`/`max_output` and deprecation notices.

## Wire mapping
Endpoint, auth header, request body, message shapes (with `reasoning_content`), and chunk → `RawEvent` mapping.

### `usage` mapping
How DeepSeek's `prompt_cache_hit_tokens` and `reasoning_tokens` map onto `Usage`.

## Examples
The DeepSeek example targets (`deepseek`, `thinking`) and their prerequisites.
