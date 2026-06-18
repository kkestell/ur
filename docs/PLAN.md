# `ur` phased implementation plan

This plan implements the public contract in `API.md` and the DeepSeek-specific contract in `DEEPSEEK.md`. The phase descriptions intentionally avoid restating every API detail; `API.md` remains the source of truth for exact public signatures and semantics.

## Global completion criteria

The project is complete when all of these hold:

- `cargo test --workspace --all-features` passes.
- `cargo +1.85 test --workspace --all-features` passes, proving the documented MSRV still builds as dependencies drift.
- `cargo clippy --workspace --all-features --all-targets -- -D warnings` passes.
- `cargo fmt --all --check` passes.
- `cargo doc --workspace --all-features --no-deps` builds without public API breakage or broken intra-doc links.
- Public API snapshots or compile-fail/compile-pass tests prove the exported surface matches `API.md` for `ur`, `ur-core`, `ur-macros`, and `ur-deepseek`.
- Unit and integration tests cover the agent loop invariants: complete-turn-only history commits, rollback on error/drop, reasoning retention, tool-call accumulation, sequential tool execution, unknown-tool behavior, and terminal `Done` emission.
- Provider tests cover DeepSeek request encoding, SSE decoding, retry/error mapping, strict tool normalization, builder validation, and model catalog lookup without requiring live network access.
- Optional live DeepSeek smoke tests are ignored by default and run only when `DEEPSEEK_API_KEY` is set.

## Phase 1: Workspace and crate skeleton

Status: Complete.

Build:

- Create the Cargo workspace with crates `ur`, `ur-core`, `ur-macros`, and `ur-deepseek`.
- Configure edition 2024, MSRV 1.85, license metadata, feature wiring, and default features as documented.
- Add shared dependencies only where needed: `futures-core`, `futures-util` for tests/examples, `serde`, `serde_json`, `schemars`, `tracing`, `thiserror` or handwritten errors, `proc-macro2`/`quote`/`syn`, `reqwest`, `tokio`, and an HTTP mocking crate for provider tests. Keep runtime and HTTP dependencies out of `ur-core`; `tokio`, `reqwest`, and HTTP mocks belong only in provider crates, integration tests, or examples.
- Add baseline CI-friendly commands through docs or scripts if desired, but do not require custom tooling to run the standard checks.

Done when:

- `cargo metadata --no-deps` succeeds for the workspace.
- `cargo test --workspace --all-targets` succeeds with placeholder crates.
- `cargo test -p ur --no-default-features` succeeds, proving the facade compiles without any provider enabled.
- `cargo doc --workspace --no-deps` succeeds.
- The facade has the intended feature graph: `serde` default-on, `deepseek` default-on, and provider-free builds still expose the core API.
- `cargo tree -p ur-core -e normal` contains no `tokio` or `reqwest`, proving the provider-agnostic core stays runtime/HTTP-agnostic.

## Phase 2: Core data model, settings, and errors

Status: Complete.

Build:

- Implement `ur-core` public aliases, `Error`, `Result`, `UserMessage`, model/settings enums, `Usage`, `FinishReason`, `ToolOutput`, `ToolSchema`, `ToolArguments`, `MessageRole`, `ToolCall`, `Message`, `Request`, `RawEvent`, `ModelSpec`, and `ModelNotice`.
- Implement `Message` as the foundational conversation record: system/user constructors, assistant content plus optional `reasoning_content` and tool calls, tool `tool_call_id` plus content, and per-role read accessors matching `API.md`.
- Implement serde derives behind the `serde` feature exactly where `API.md` specifies them.
- Implement `Display`, `std::error::Error`, `Debug`, `Clone`, `Default`, `PartialEq`, and `Eq` behavior required by the spec.
- Implement `Provider` and blanket `Provider for Arc<T>`.
- Implement `Tool` and blanket `Tool for Arc<T>`.

Done when:

- Unit tests prove `ToolArguments` constructs from `new`, `String`, and `&str`; is transparent under serde; preserves raw JSON text; parses to typed values; parses to `JsonValue`; and displays as the raw JSON string.
- Unit tests prove `Error::source()` exposes sources only for `Transport` and `Decode`.
- Unit tests prove `UserMessage` converts from `&str` and `String`, exposes `as_str()`, derives the required traits including `Hash`, has no `Default`, and serializes/deserializes under the `serde` feature.
- Unit tests prove `ToolOutput` maps from `Result<String, String>`, exposes borrowed result/content accessors, and serializes to the documented `{ "status", "content" }` shape under the `serde` feature.
- Unit tests prove `Message` and `ToolCall` constructors and read accessors expose exactly the role, content, reasoning content, tool calls, and tool-call ids required by providers.
- Compile tests prove `Provider` and `Tool` are object-safe and usable behind `Arc<dyn Provider>` / `Arc<dyn Tool>`.
- Feature tests prove the crate compiles with and without the facade `serde` feature, and serde impls are present only when enabled.
- Public type trait tests prove all required `Debug`, `Clone`, `Default`, equality, and `Send + Sync + 'static` invariants.

## Phase 3: Model, Agent, Session state, and request construction

Status: Complete.

Build:

- Implement `Model<P>` with provider-bound model id, read-only catalog facts, chainable generation settings, and local `max_tokens` validation rules.
- Resolve `Provider::model_spec` and `Provider::model_notice` exactly once in `Model::new`, cache the spec result on the model, and emit any `ModelNotice::Deprecated` as one `tracing` warning; `context_window()`, `max_output()`, request construction, and `max_tokens` validation must use the cached spec rather than re-querying the provider.
- Implement `Agent<P>` registration of one or many tools, preserving tool schemas and tool lookup by name.
- Implement `Session<P>` initialization, read-only history, reset, and request construction with system prompt, full prior history, tools, and model settings.
- Add manual opaque `Debug` impls for `Model`, `Agent`, `Session`, and later `EventStream` that do not require `P: Debug`.

Done when:

- Tests with a fake provider prove known model ids expose `context_window()` and `max_output()`, unknown ids return `None`, and unknown ids remain constructible.
- Tests with a counting fake provider prove `Model::new` performs one catalog lookup and one notice lookup per model construction, and accessors/settings/request construction do not call `model_spec` or `model_notice` again.
- Tests prove `max_tokens(0)` and over-catalog-cap values surface as `Error::Config` through `send`, before provider execution.
- Tests prove an unknown model id has no local `max_tokens` upper cap: only `max_tokens(0)` is rejected locally.
- Tests prove other settings are carried verbatim in `Request.settings` and not interpreted by core.
- Tests prove `Agent::session()` is cheap and independent: two sessions from one agent do not share mutable conversation history.
- Tests prove `Session::reset()` drops all turns after the system prompt and keeps the session reusable.
- Tests prove tool schemas appear in requests in registration order and duplicate or invalid tool names surface as `Error::Config` from `send` before provider execution. This keeps `Agent::tool` infallible as specified.

## Phase 4: EventStream and provider-agnostic agent loop

Status: Complete.

Build:

- Implement `Event`, `EventStream<'_>`, and the full `Session::send` state machine.
- Consume normalized `RawEvent` streams, accumulate assistant content, reasoning content, and indexed tool-call fragments.
- On `FinishReason::ToolCalls`, emit assembled `ToolCall` events, run tools sequentially in call order, emit `ToolResult`, append tool messages, and continue the model loop.
- Commit pending history only after the final non-tool `Done`; roll back on provider error, stream cancellation, or any other non-terminal failure.
- Preserve assistant `reasoning_content` in committed history on every assistant turn.

Done when:

- Tests prove event ordering for a tool round is: model deltas, optional usage, `ToolCall`, `ToolResult`, next model deltas, final `Usage` if present, final `Done`.
- Tests prove `Event::Done` is not yielded for intermediate `ToolCalls` finishes and is yielded exactly once for the complete user turn.
- Tests prove tool-call argument fragments concatenate by `index`, with `id` and `name` taken from the first fragment.
- Tests prove multiple tool calls in one model turn run sequentially by ascending call order and append matching tool messages.
- Tests prove an unknown tool yields a `ToolOutput::Err`, appends a tool message with the error content, and does not panic.
- Tests prove malformed arguments for a macro-generated tool yield a `ToolOutput::Err`, append a tool message with the error content, and let the model retry instead of rolling the turn back.
- Tests prove successful tool return values are JSON strings and tool errors are stringified into the tool result content.
- Tests prove provider errors stop the stream and leave `Session::history()` exactly as it was before `send`.
- Tests prove dropping `EventStream` before terminal `Done` rolls history back exactly.
- Tests prove complete turns commit user, assistant, and tool messages atomically, including reasoning content.

## Phase 5: `#[ur::tool]` macro

Status: Complete.

Build:

- Implement the `ur-macros` attribute macro for async and sync functions.
- Generate a same-identifier tool value/type, preserve visibility and allowed attributes, derive parameter deserialization/schema, and implement the `Tool` trait through an absolute crate root that defaults to `::ur`. This means normal macro users must have the facade crate in scope as `ur`; tests that exercise successful expansion and registration must run from the facade or a dedicated crate that depends on the facade, not from `ur-core` or `ur-macros`.
- Support optional `description`, optional `name`, and parameter descriptions.
- Support return types `T` and `Result<T, E>` with success JSON serialization and error stringification.
- Validate macro inputs with clear compile errors: unsupported argument patterns, unsupported return shapes, invalid tool names, invalid attribute keys, and non-schema/non-deserializable parameters.

Done when:

- Compile-fail tests in `ur-macros` cover validation errors rejected by the macro before generated `::ur` paths need to type-check: invalid names, unknown macro attributes, unsupported argument patterns, unsupported return shapes, and unsupported function signatures.
- Path-independent macro tests prove parsing, generated identifiers, attribute forwarding decisions, and schema-shaping logic where that logic can be tested without depending on `ur`.
- Defer successful expansion tests, semantic compile-fail tests for generated trait bounds, runtime tool-call tests, schema assertions against the real `ToolSchema`, and `agent.tool(add)` registration tests to Phase 6, where the `ur` facade exists and can be a real dependency. Do not add `ur` as a dependency of `ur-core` or `ur-macros`.

## Phase 6: Facade crate and public API lock

Status: Complete.

Build:

- Re-export the provider-agnostic surface from `ur-core`, the `#[ur::tool]` macro from `ur-macros`, public dependency aliases, and enabled provider crates under their facade modules.
- Wire feature flags so `ur` compiles provider-free and with DeepSeek enabled by default.
- Add examples that match `API.md` and `DEEPSEEK.md`, using `futures_util::StreamExt`.
- Add public API lock tests using compile-pass examples and, if practical, `rustdoc`/`trybuild` assertions rather than brittle textual snapshots.
- Add the macro pass/runtime tests that require `::ur` to resolve: the `API.md` macro examples, `agent.tool(add)` registration, sync and async invocation, `Option<T>` optionality, successful output JSON serialization, error stringification, generated schema assertions, and preservation of visibility/doc comments/`#[cfg]`/ordinary item attributes.

Done when:

- `cargo test -p ur --no-default-features` passes and examples using only a fake provider compile.
- `cargo test -p ur --all-features` passes and `ur::deepseek::DeepSeekClient` is available only when the `deepseek` feature is enabled.
- The complete examples in `API.md` and `DEEPSEEK.md` compile as doctests or example targets.
- `trybuild` pass tests prove the `#[ur::tool]` examples from `API.md` compile through the facade and can be registered with `agent.tool(add)`.
- `trybuild` compile-fail tests through the facade prove non-serializable returns and non-deserializable/non-schema parameters fail with clear diagnostics.
- Runtime facade tests prove macro-generated sync and async tools deserialize arguments, apply `Option<T>` optionality, serialize successful outputs, stringify errors, and expose the expected real `ToolSchema`.
- Tests prove visibility, doc comments, `#[cfg]`, and ordinary item attributes are preserved on the generated macro item.
- Public signatures use `ur::BoxFuture`, `ur::BoxStream`, `ur::JsonSchema`, `ur::JsonValue`, and `ur::JsonError` where the spec says callers should see those names.

## Phase 7: DeepSeek client, catalog, and request encoding

Build:

- Implement `ur-deepseek` client handle, builder, HTTP-client wrapper, environment constructors, builder validation, opaque `Debug`, and cheap `Clone`.
- Implement `Provider::model_spec` as a pure compiled-in DeepSeek catalog lookup and `Provider::model_notice` as a pure lookup that returns `ModelNotice::Deprecated` for `deepseek-chat` and `deepseek-reasoner`. Do not log from either provider lookup; `Model::new` emits the warning from the returned notice.
- Convert core `Request` values into DeepSeek `POST /chat/completions` JSON, including full history, reasoning content, tool declarations, strict mode normalization, settings, `stream: true`, and `stream_options.include_usage: true`.
- Enforce provider-local validation: API key availability, URL validity, `user_id`, `temperature`, `top_p`, `stop` count, strict-mode beta requirement, strict/non-strict mixing policy, and DeepSeek `max_tokens` cap.
- Omit `temperature` and `top_p` unless thinking is explicitly disabled, and map reasoning effort aliases as documented.

Done when:

- Builder tests prove env fallback, explicit API key override, `beta(true)` URL selection, explicit base URL precedence, timeout/retry overrides, invalid URL rejection, missing API key rejection, and `user_id` validation.
- Catalog tests prove all documented ids return `ModelSpec { context_window: 1_000_000, max_output: 384_000 }` and unknown ids return `None`.
- Tests prove constructing a model with each legacy id emits exactly one warning, while direct calls to `Provider::model_spec` and `Provider::model_notice` remain silent and repeatable.
- Request-encoding tests compare JSON bodies for no-tool, tool, strict-tool, reasoning, JSON response, stop, and user-id cases.
- Tests prove reasoning content stored in core `Message` is serialized into assistant messages on every follow-up request.
- Tests prove strict mode rewrites accepted schemas into DeepSeek's strict subset, rejects strict mode without beta, rejects mixed strict/non-strict tool sets locally, and emits `strict: true` for every tool when the accepted set is strict.
- Tests prove invalid generation settings return `Error::Config` before any HTTP request is sent.

## Phase 8: DeepSeek streaming, retries, and error mapping

Build:

- Implement streaming-only HTTP execution with SSE parsing.
- Ignore SSE comments such as `: keep-alive`, ignore blank lines, stop on `data: [DONE]`, and decode each JSON `data:` chunk to `RawEvent`.
- Map chunk content, reasoning content, tool-call deltas, finish reasons, and usage to the normalized provider seam.
- Implement retry policy for retryable statuses and transient transport failures, including `Retry-After` handling for rate limits and exponential backoff bounded by `max_retries`.
- Map HTTP statuses and decode/transport failures into the shared `Error` vocabulary.

Done when:

- SSE parser tests cover text deltas, reasoning deltas, multi-fragment tool calls, usage-only final chunks with empty choices, finish reasons, keep-alive comments, blank lines, `[DONE]`, malformed JSON, and unknown finish reasons.
- HTTP mock tests prove request headers, endpoint path, timeout behavior where testable, and body shape.
- Retry tests prove 408/429/500/502/503/504 and retryable transport failures retry up to `max_retries`, while 400/401/402/422, TLS-like terminal errors, and decode errors fail immediately.
- Error mapping tests prove 401 maps to parameterless `Auth`, 402 to parameterless `InsufficientFunds`, 400 to `BadRequest`, 422 to `InvalidParams`, 429 to `RateLimited`, retryable server statuses to `Server`, and malformed streams to `Decode`.
- Provider contract tests prove each successful stream terminates with exactly one `RawEvent::Done`, and provider errors are emitted as `Err(Error)` items.

## Phase 9: Integration, docs, and release hardening

Build:

- Add workspace-level integration tests that exercise `ur` facade + `ur-core` + `ur-macros` + fake provider together.
- Add ignored live DeepSeek tests for a small text-only request and, if feasible, a simple tool-call request.
- Add crate docs and examples that point to `API.md`/provider docs without duplicating all internals.
- Audit public API against `API.md` and update either code or docs for any discovered mismatch before release.

Done when:

- The global completion commands all pass.
- A fake-provider integration test demonstrates the complete provider-agnostic example from `API.md`.
- An HTTP-mocked DeepSeek integration test demonstrates a full tool round trip through `Session::send`.
- Ignored live tests are documented, require `DEEPSEEK_API_KEY`, and do not run in normal CI.
- Every invariant from `API.md` that is behavioral rather than purely documentary has at least one focused unit, compile, integration, or provider test.
- The remaining known gaps, if any, are documented explicitly as deferred work rather than implicit TODOs, including the pre-1.0 stability decision for `DeepSeekHttpClient::from_reqwest(reqwest::Client)`.

## Suggested implementation order

Implement phases in order. Do not start DeepSeek network behavior before the provider-agnostic loop is covered by fake-provider tests; otherwise provider bugs and loop bugs will be hard to distinguish. Keep each phase mergeable by maintaining passing tests for completed crates and marking future-provider tests as pending only until their phase begins.
