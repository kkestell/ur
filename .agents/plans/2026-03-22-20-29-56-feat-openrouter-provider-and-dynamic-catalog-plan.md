---
title: "feat: Add OpenRouter provider and dynamic catalog"
type: feat
date: 2026-03-22
---

# Add OpenRouter provider and dynamic catalog

## Overview

Add a new built-in `llm-openrouter` extension that talks to OpenRouter's chat completions API, supports streaming and tool calls, and discovers its model catalog dynamically from `GET /api/v1/models` instead of hardcoding a static list.

The implementation must make the dynamic catalog usable in the existing `ur model` flows. That means we need to handle OpenRouter's slash-containing model IDs, initialize providers with API keys during `model` commands, translate OpenRouter's `supported_parameters` into per-model settings, and add a live smoke test that proves `tool call -> tool result -> second streamed response` with `OPENROUTER_API_KEY`.

No relevant brainstorm or `.agents/solutions/` document was found for this feature, so this plan is based on local repo research plus current OpenRouter docs.

## Why This Matters

- `ur` currently has one real networked LLM provider, [`extensions/system/llm-google/src/lib.rs`](/home/kyle/src/ur/extensions/system/llm-google/src/lib.rs#L80), and it assumes a static, curated model list.
- OpenRouter is the opposite shape: the provider surface is large, changes over time, and model capabilities must be discovered at runtime.
- The existing model/config plumbing is close, but it has three hard blockers for OpenRouter:
  - [`src/config.rs`](/home/kyle/src/ur/src/config.rs#L105) rejects model IDs containing `/`, while OpenRouter model IDs are slugs like `openai/gpt-4`.
  - [`src/model.rs`](/home/kyle/src/ur/src/model.rs#L47) initializes providers with `init(&[])`, so a provider that needs an API key to implement `list_models()` cannot participate in `model list`, `model get`, `model set`, or `model config`.
  - [`wit/world.wit`](/home/kyle/src/ur/wit/world.wit#L58) only supports integer, enum, and boolean settings. OpenRouter's common parameters include floats such as `temperature`, `top_p`, `frequency_penalty`, and `presence_penalty`.

## Research Findings

### Internal References

- [`extensions/system/llm-google/src/lib.rs`](/home/kyle/src/ur/extensions/system/llm-google/src/lib.rs#L84) is the provider template to mirror for `init`, `provider_id`, `list_models`, `complete`, and `complete_streaming`.
- [`src/model.rs`](/home/kyle/src/ur/src/model.rs#L47) owns provider discovery, role resolution, and all `ur model` CLI handlers.
- [`src/config.rs`](/home/kyle/src/ur/src/config.rs#L75) validates and materializes typed provider settings from `ModelDescriptor.settings`.
- [`src/turn.rs`](/home/kyle/src/ur/src/turn.rs#L254) already has the env-to-provider init pattern, but only for Google.
- [`scripts/smoke_test/harness.py`](/home/kyle/src/ur/scripts/smoke_test/harness.py#L23) is where new built-in extension artifacts must be built and copied for smoke runs.
- [`scripts/smoke_test/test_google_provider.py`](/home/kyle/src/ur/scripts/smoke_test/test_google_provider.py#L58) is the live-provider smoke test pattern to mirror.
- [`Makefile`](/home/kyle/src/ur/Makefile#L8) only builds/checks/clippy-tests the current built-in system extensions, so OpenRouter must be added there too.

### External Docs (verified 2026-03-22)

- OpenRouter model catalog docs: <https://openrouter.ai/docs/api-reference/models/get-models>
  - `GET https://openrouter.ai/api/v1/models`
  - Requires bearer auth
  - Supports `supported_parameters` and `output_modalities` query filters
  - Response includes `id`, `name`, `description`, `pricing`, `context_length`, `top_provider.max_completion_tokens`, `architecture`, and `supported_parameters`
- OpenRouter chat completions docs: <https://openrouter.ai/docs/api/api-reference/chat/send-chat-completion-request>
  - `POST /api/v1/chat/completions`
  - Supports `stream`, `tools`, `tool_choice`, `parallel_tool_calls`, `max_completion_tokens`, `max_tokens`, `temperature`, `top_p`, `frequency_penalty`, `presence_penalty`, `reasoning`, and more
  - Non-streaming responses use OpenAI-style `message.tool_calls`
  - Streaming responses use `choices[].delta.content` and `choices[].delta.tool_calls`
- OpenRouter streaming docs: <https://openrouter.ai/docs/api/reference/streaming>
  - Streaming is SSE
  - Streams may contain keepalive comment lines like `: OPENROUTER PROCESSING`
  - Mid-stream failures arrive as SSE `data:` events with a top-level `error` object and `finish_reason: "error"`
- OpenRouter tool-calling docs: <https://openrouter.ai/docs/guides/features/tool-calling>
  - Tool-capable models can be filtered by `supported_parameters=tools`
  - Tool follow-up requests use an assistant message with `tool_calls`, then a `role: "tool"` message with `tool_call_id`
  - The `tools` array must be sent on both the initial and follow-up requests
- OpenRouter parameters docs: <https://openrouter.ai/docs/api/reference/parameters>
  - `tools` and `max_tokens` automatically restrict routing to compatible providers
  - For other parameters, `provider.require_parameters = true` is the mechanism that prevents silent parameter dropping

**Inference from docs:** OpenRouter's model-catalog `supported_parameters` should be treated as a capability hint, not a perfect guarantee for every routed provider path. To keep `ur`'s model settings trustworthy, the OpenRouter provider should set `provider.require_parameters = true` whenever it relies on optional parameters or tool calling.

## Proposed Solution

Implement this as four focused slices.

### Phase 1: Host and Model-Plumbing Prerequisites

#### 1. Allow slash-containing model IDs

OpenRouter model IDs already include a provider/author prefix, e.g. `openai/gpt-4o-mini`. That means the full `ur` model reference becomes `openrouter/openai/gpt-4o-mini`.

Update [`src/config.rs`](/home/kyle/src/ur/src/config.rs#L105) so `parse_model_ref()` splits on the first slash only:

- Provider ID = everything before the first slash
- Model ID = everything after the first slash, including additional slashes

This change must be covered by tests for:

- `openrouter/openai/gpt-4o-mini` -> valid
- `openrouter/google/gemini-2.5-flash` -> valid
- `/model` -> invalid
- `provider/` -> invalid
- `justprovider` -> invalid

Also add config round-trip tests to prove TOML serialization/deserialization preserves slash-containing model IDs in:

- `roles.default = "openrouter/openai/gpt-4o-mini"`
- `[providers.openrouter."openai/gpt-4o-mini"]`

#### 2. Reuse provider init config in `model` commands

Today [`src/model.rs`](/home/kyle/src/ur/src/model.rs#L47) initializes providers with `init(&[])`, which is incompatible with an API-backed `list_models()`.

Refactor the current helper in [`src/turn.rs`](/home/kyle/src/ur/src/turn.rs#L254) into a shared function that both:

- `turn::run()` uses before completions
- `model::collect_provider_models()` uses before `provider_id()` and `list_models()`

The shared helper should map:

- `google` -> `GOOGLE_API_KEY`
- `openrouter` -> `OPENROUTER_API_KEY`

This keeps the provider lifecycle consistent and lets `ur model list|get|set|config|setting|info` work with live, authenticated providers.

#### 3. Extend typed settings to support floating-point values

OpenRouter's commonly exposed model settings are mostly numeric floats. Extend the WIT and host config pipeline to support a `number` type:

In [`wit/world.wit`](/home/kyle/src/ur/wit/world.wit#L58):

- Add `record setting-number { min: float64, max: float64, default-val: float64 }`
- Add `number(setting-number)` to `setting-schema`
- Add `number(float64)` to `setting-value`

Then update:

- [`src/config.rs`](/home/kyle/src/ur/src/config.rs#L75) to parse, validate, store, and materialize number settings from TOML floats
- [`src/model.rs`](/home/kyle/src/ur/src/model.rs#L172) to display number schemas in `ur model config`
- Unit tests for parsing, validation, defaults, and round-trip persistence

This is the minimum schema expansion needed to expose OpenRouter's practical scalar parameters without inventing stringified or scaled-int hacks.

### Phase 2: New `llm-openrouter` Built-In Extension

Add a new built-in extension crate:

```text
extensions/system/llm-openrouter/
├── Cargo.toml
└── src/lib.rs
```

Use the same `llm-extension-http` world as Google. Keep the provider self-contained; do not build a generic shared HTTP abstraction in v1.

#### Extension lifecycle

- `id()` -> `"llm-openrouter"`
- `name()` -> `"OpenRouter"`
- `provider_id()` -> `"openrouter"`
- `init(config)` reads `api_key`
- `list_tools()` remains empty
- `call_tool()` returns unsupported, same as other LLM providers

#### Catalog fetch for `list_models()`

`list_models()` should perform a live authenticated fetch against:

```text
GET /api/v1/models?supported_parameters=tools&output_modalities=text
```

Then locally filter the response down to models `ur` should actually expose:

- Model `id` and `name` are non-empty
- `supported_parameters` contains `tools`
- `architecture.input_modalities` contains `text`
- `architecture.output_modalities` contains `text`
- A usable input context window exists
- A usable output token limit exists via `top_provider.max_completion_tokens` or a safe fallback

Sort the resulting list deterministically by model ID before marking defaults. This avoids catalog-order flakiness in tests and config diffs.

#### Provider-internal metadata model

Create provider-private structs for the OpenRouter catalog payload, including:

- Raw `supported_parameters`
- Raw `default_parameters`
- `pricing`
- `architecture`
- `context_length`
- `top_provider.max_completion_tokens`

Do **not** mirror raw `supported_parameters` into WIT in v1. Instead:

- Capture the raw list internally
- Use it to decide which `SettingDescriptor`s to publish
- Use it when validating/mapping request settings

This keeps the host/provider contract stable and uses the existing `ModelDescriptor.settings` path the CLI already understands.

#### Metadata mapping

Translate OpenRouter catalog fields into `ModelDescriptor`:

- `id` -> unchanged OpenRouter slug, e.g. `openai/gpt-4o-mini`
- `name`, `description` -> direct from catalog
- `context_window_in` -> `context_length`
- `context_window_out` -> `top_provider.max_completion_tokens` when present, otherwise a conservative fallback
- `knowledge_cutoff` -> `"unknown"` for now unless the catalog exposes a reliable date field worth surfacing later
- `cost_in` / `cost_out` -> convert OpenRouter pricing strings into existing `millidollars per million tokens`

Pricing conversion must be explicit in the implementation notes and tests. OpenRouter returns decimal strings in dollars-per-token; `ur` stores millidollars-per-million-tokens.

#### Default-model selection

Mark exactly one filtered model as `is_default: true`.

Use a deterministic rule:

1. Prefer the first present model from a short ordered shortlist of known tool-capable defaults
2. Otherwise fall back to the first filtered model after sorting

The shortlist should stay provider-internal so smoke tests do not depend on hardcoded external model IDs.

### Phase 3: OpenRouter Completion and Streaming Support

#### Request translation

Build OpenAI-compatible request bodies from `ur` message parts:

- User/system/assistant text -> standard `messages[]`
- Assistant tool calls -> assistant message with `tool_calls`
- Tool results -> `role: "tool"` message with `tool_call_id` and JSON/text content
- `ToolDescriptor` -> OpenAI/OpenRouter `tools[]` function definitions

Every request should include:

- `model`
- `messages`
- `tools` when tools are available
- `provider.require_parameters = true`

That last field is important: it prevents settings/tool support from being silently ignored when OpenRouter routes among underlying providers.

#### Supported setting mapping

Map OpenRouter raw capabilities into a curated v1 settings surface:

| OpenRouter capability | `ur` setting key | Type |
|---|---|---|
| `max_tokens` or `max_completion_tokens` | `max_output_tokens` | integer |
| `temperature` | `temperature` | number |
| `top_p` | `top_p` | number |
| `frequency_penalty` | `frequency_penalty` | number |
| `presence_penalty` | `presence_penalty` | number |
| `seed` | `seed` | integer |
| `parallel_tool_calls` | `parallel_tool_calls` | boolean |

Rules:

- Only expose a setting if the raw model metadata says it is supported
- Use `default_parameters` when present; otherwise fall back to provider defaults
- Defer structured parameters like `response_format`, `reasoning`, `tool_choice`, `stop`, and `logit_bias` to follow-up work

This keeps the CLI useful without overextending the WIT/config layer into arbitrarily nested JSON values.

#### Non-streaming `complete()`

Implement `complete()` against `POST /api/v1/chat/completions` with `stream = false`.

Parse:

- `choices[0].message.content`
- `choices[0].message.tool_calls`
- `choices[0].finish_reason`
- `usage.prompt_tokens` / `usage.completion_tokens`

Map assistant tool calls back into `ur::extension::types::ToolCall` parts.

#### Streaming `complete_streaming()`

Implement `complete_streaming()` against `POST /api/v1/chat/completions` with `stream = true`.

The SSE parser must:

- Ignore comment lines such as `: OPENROUTER PROCESSING`
- Ignore blank events cleanly
- Parse `data:` JSON events incrementally
- Detect and surface mid-stream `error` events
- Accumulate tool-call deltas across chunks until the tool call is complete
- Emit usage stats from the final chunk when present

Follow the same resource-based pattern as Google:

- internal `RefCell` state
- incremental `next()` pulls
- robust handling of partial SSE events across reads

### Phase 4: Build/Smoke Wiring and UX Proof

#### Built-in extension wiring

Update:

- [`Makefile`](/home/kyle/src/ur/Makefile#L8) to build/check/clippy the new built-in crate
- [`scripts/smoke_test/harness.py`](/home/kyle/src/ur/scripts/smoke_test/harness.py#L23) to build and install the new WASM artifact

#### Smoke test module

Add:

```text
scripts/smoke_test/test_openrouter_provider.py
```

Wire it into:

- [`scripts/smoke_test/__init__.py`](/home/kyle/src/ur/scripts/smoke_test/__init__.py#L3)
- `scripts/smoke-test.py`

#### Smoke flow

The smoke should avoid hardcoding a volatile OpenRouter model slug. Use the provider's own default selection instead:

1. Skip if `OPENROUTER_API_KEY` is unset
2. Enable `test-extension`
3. Disable `llm-google` so OpenRouter becomes the only real `llm-provider`
4. Run `ur model get default` and assert it resolves to `openrouter/...`
5. Run `ur model config default` to show the dynamically derived settings surface
6. Run `ur run` with the Paris weather prompt
7. Assert stdout shows:
   - `resolving role "default" -> openrouter/...`
   - `LLM returned tool call: get_weather(`
   - `tool result:`
   - `calling LLM streaming (... includes tool result)`
   - a final answer mentioning Paris / coat
8. Re-enable Google and disable `test-extension` in `finally`

This proves:

- dynamic default-model selection
- streaming output
- tool-call parsing
- tool-result replay
- second completion with tool results

## SpecFlow Scenarios

### Scenario 1: Slashy OpenRouter model refs work end-to-end

- Given a live catalog entry with ID `openai/gpt-4o-mini`
- When the user runs `ur model set default openrouter/openai/gpt-4o-mini`
- Then the config is saved successfully
- And `ur model get default`
- And `ur model info openrouter/openai/gpt-4o-mini cost_in`
- And `ur run`
- all resolve the same model ID without truncation or parse failure

### Scenario 2: Dynamic model settings are capability-driven

- Given a filtered OpenRouter model whose raw `supported_parameters` includes `temperature` and `top_p` but not `frequency_penalty`
- When the user runs `ur model config default`
- Then `temperature` and `top_p` appear
- And `frequency_penalty` does not
- And `ur model setting default frequency_penalty 1.0` fails before any API call

### Scenario 3: Authenticated model commands work

- Given `OPENROUTER_API_KEY` is present in the environment
- When the user runs `ur model list`, `ur model get default`, or `ur model config default`
- Then the OpenRouter provider initializes successfully and contributes live catalog data

### Scenario 4: Streamed tool use completes the full loop

- Given `test-extension` is enabled and the selected OpenRouter default model supports tools
- When the user asks the Paris weather question
- Then the first streamed response yields one or more tool calls
- And the tool result is appended as a `role: "tool"` message
- And the second streamed response returns the final text answer

### Scenario 5: SSE noise and failures are handled cleanly

- Given OpenRouter inserts keepalive comment lines during streaming
- Then the parser ignores them
- And given OpenRouter emits a mid-stream error event
- Then the provider returns a clear error instead of hanging or emitting malformed chunks

## Engineering Quality

| Principle | Application |
|---|---|
| **SRP** | Keep catalog fetch/filter, settings mapping, request translation, and SSE parsing in separate helpers inside `llm-openrouter`; keep slash-model-ref parsing and typed-setting handling in host modules. |
| **OCP / DIP** | The host continues to depend on WIT `ModelDescriptor` and `ConfigSetting`; OpenRouter-specific raw response fields stay inside the extension. |
| **YAGNI / KISS** | Do not add a generic provider SDK, persistent catalog cache, or raw JSON setting editor in v1. Ship the smallest slice that makes the catalog usable and the smoke test real. |
| **Value Types** | Add a true WIT `number` setting type instead of encoding floats as strings or scaled integers. |
| **TDD** | Add focused unit tests before implementation for slashy model refs, number settings, catalog filtering/mapping, response parsing, SSE comments, and tool-call deltas. |

## Acceptance Criteria

- [ ] New built-in crate `extensions/system/llm-openrouter` builds for `wasm32-wasip2` and exports `extension`, `llm-provider`, and `llm-streaming-provider`
- [ ] `ur model` commands initialize `openrouter` with `OPENROUTER_API_KEY` and can call live `list_models()`
- [ ] `parse_model_ref()` accepts `openrouter/<author>/<slug>` model refs and config round-trips them correctly
- [ ] WIT/host config/model CLI support number settings in addition to integer, enum, and boolean
- [ ] `list_models()` fetches `GET /api/v1/models` dynamically, filters to tool-capable text models, and returns deterministic descriptors
- [ ] OpenRouter raw `supported_parameters` are captured internally and drive the per-model `settings` descriptors exposed to the CLI
- [ ] OpenRouter metadata populates `context_window_in`, `context_window_out`, `cost_in`, and `cost_out` with tested conversions
- [ ] Requests use `provider.require_parameters = true` so configured settings/tool use are not silently ignored by routing
- [ ] Non-streaming and streaming completions both translate `ur` messages/tools to OpenRouter format and parse assistant tool calls back into `MessagePart::ToolCall`
- [ ] Streaming parser ignores SSE comments and handles mid-stream error events cleanly
- [ ] Live smoke test skips without `OPENROUTER_API_KEY`, otherwise demonstrates streaming + tool calling end-to-end against OpenRouter
- [ ] `make check`, `make test`, `make clippy`, and `make smoke-test` pass

## Implementation Order

1. Extend typed settings with `number` support and add host-side tests
2. Fix slash-containing model refs in config/model parsing and add round-trip tests
3. Extract shared provider init config and make `model::collect_provider_models()` use it
4. Scaffold `extensions/system/llm-openrouter`
5. Implement dynamic catalog fetch/filter/mapping and default-model selection
6. Implement non-streaming request/response translation
7. Implement streaming SSE parsing with tool-call deltas and error handling
8. Wire the new built-in into `Makefile` and smoke harness
9. Add the OpenRouter smoke test and run full verification

## Risks and Mitigations

- **OpenRouter catalog size and churn**
  - Mitigation: use server-side `supported_parameters=tools&output_modalities=text` filtering plus deterministic local sorting
- **`supported_parameters` may overstate what a routed provider path supports**
  - Mitigation: send `provider.require_parameters = true` and keep the v1 settings surface to explicit scalar mappings
- **Slash-containing model IDs may break hidden assumptions beyond `parse_model_ref()`**
  - Mitigation: add unit tests around `model set`, `model info`, config save/load, and role resolution
- **Streaming may include non-JSON SSE payloads or unified error events**
  - Mitigation: parser must explicitly skip comment frames and recognize top-level `error`
- **No reliable catalog knowledge-cutoff field**
  - Mitigation: use `"unknown"` in v1 instead of inventing fake metadata

## References

### Internal

- [`extensions/system/llm-google/src/lib.rs`](/home/kyle/src/ur/extensions/system/llm-google/src/lib.rs#L84)
- [`src/model.rs`](/home/kyle/src/ur/src/model.rs#L47)
- [`src/config.rs`](/home/kyle/src/ur/src/config.rs#L75)
- [`src/turn.rs`](/home/kyle/src/ur/src/turn.rs#L254)
- [`wit/world.wit`](/home/kyle/src/ur/wit/world.wit#L58)
- [`scripts/smoke_test/harness.py`](/home/kyle/src/ur/scripts/smoke_test/harness.py#L23)
- [`scripts/smoke_test/test_google_provider.py`](/home/kyle/src/ur/scripts/smoke_test/test_google_provider.py#L58)
- [`Makefile`](/home/kyle/src/ur/Makefile#L8)

### External

- OpenRouter models API: <https://openrouter.ai/docs/api-reference/models/get-models>
- OpenRouter chat completions API: <https://openrouter.ai/docs/api/api-reference/chat/send-chat-completion-request>
- OpenRouter streaming guide: <https://openrouter.ai/docs/api/reference/streaming>
- OpenRouter tool-calling guide: <https://openrouter.ai/docs/guides/features/tool-calling>
- OpenRouter parameters guide: <https://openrouter.ai/docs/api/reference/parameters>
