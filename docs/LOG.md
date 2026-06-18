## Phase 1

- Created a Cargo workspace with `ur`, `ur-core`, `ur-macros`, and `ur-deepseek` crates under `crates/`.
- Set shared package metadata to Kyle Kestell <kyle@kestell.org>, repository/homepage `https://github.com/kkestell/ur`, edition 2024, MSRV 1.85, and `MIT OR Apache-2.0`.
- Kept `ur-core` runtime-agnostic: its normal dependency tree has no `tokio` or `reqwest`. Runtime and HTTP dependencies start in `ur-deepseek`.
- Wired the `ur` facade with default features `serde` and `deepseek`; `cargo test -p ur --no-default-features` verifies provider-free facade builds still compile.
- Kept `serde`, `serde_json`, and `schemars` as unconditional `ur-core` dependencies, matching `API.md`: tool support needs them, while the facade `serde` feature controls public `Serialize`/`Deserialize` impls.
- Deferred `proc-macro2`, `quote`, and `syn` until the macro implementation needs parsing and code generation.
- Added placeholder public items only where needed to prove crate/module boundaries and re-export paths. Full semantics remain deferred to later phases.
- Committed `Cargo.lock` for repeatable workspace validation. Cargo selected dependency versions compatible with Rust 1.85.

## Phase 2

- Replaced the `ur-core` placeholders with the documented core data model: `Error`, `UserMessage`, settings enums, event records, tool schema/arguments, provider request/event records, model catalog records, and conversation `Message`/`ToolCall`.
- Kept `Provider` and `Tool` object-safe and added the `Arc<T>` blanket impls required for `Arc<dyn Provider>` and `Arc<dyn Tool>`.
- Preserved the public serde feature boundary by deriving public serialization only behind the facade/core `serde` feature, while keeping serde itself available in `ur-core` for tool argument parsing.
- Added focused unit tests for tool argument parsing/display/serde transparency, error source chaining, user message conversions and traits, tool output shape, message accessors, object-safe trait-object usage, and public trait invariants.
- Added facade compile-contract tests that build small fixture crates to prove serde impls are exposed with `ur/serde`, absent without it, `UserMessage` has no `Default`, and provider-free trait-object usage compiles.
- Implemented `Hash` for `ToolSchema` with an order-stable recursive JSON hash instead of serializing `JsonValue`; this avoids violating `Eq` if another crate enables `serde_json/preserve_order`.
- Updated the DeepSeek placeholder provider to satisfy the new `Provider` seam with an empty stream and no catalog facts, deferring real DeepSeek behavior to later phases.
- Deferred agent/model/session behavior, request construction, tool registration validation, macro expansion, and provider networking to their planned phases.

## Phase 3

- Replaced the placeholder handles with real `Model<P>`, `Agent<P>`, and `Session<P>` state matching the documented constructor and builder surface.
- `Model::new` now resolves `model_spec` and `model_notice` exactly once, caches the optional `ModelSpec`, and emits deprecation notices through `tracing::warn!`; later accessors and request construction use only cached data.
- Stored providers behind `Arc<P>` inside `Model` so cloning models, agents, and sessions stays cheap and does not require `P: Clone`; public `Debug` for handles remains opaque and does not require `P: Debug`.
- Kept generation-setting builders infallible. `Session::send` performs the local `max_tokens` checks and surfaces `Error::Config` through `EventStream` before provider `chat` is called.
- Cached tool schemas at registration time so requests preserve registration order. Tool name validation, duplicate detection, and runtime-name/schema-name mismatch checks are deferred to `send`, keeping `Agent::tool` infallible.
- Implemented session initialization with a system message, read-only history access, reset to the system prompt, and request construction that stages the current user turn without committing history.
- Added a minimal `EventStream<'_>` implementation for Phase 3 preflight errors. The full provider event loop, successful-turn event emission, history commit/rollback, and tool execution remain deferred to Phase 4.
- Removed leftover placeholder-era accessors that were not part of `docs/API.md`, avoiding accidental public API expansion before the public surface is locked.
- Added coverage for registered-tool lookup by name, deprecated-model warning emission, and constructing `Model<Arc<dyn Provider>>` through the facade compile contracts after independent review flagged those gaps.

## Phase 4

- Replaced the minimal `EventStream<'_>` placeholder with the full provider-agnostic loop: staged user turns, provider stream polling, assistant delta accumulation, tool-call assembly, sequential tool execution, retrying the model after tool results, and terminal `Done` emission.
- Kept `EventStream<'_>` non-generic by erasing the session provider to `Arc<dyn Provider>` inside `Session::send`; this preserves the documented public stream type while still supporting `Model<P>` and `Model<Arc<dyn Provider>>`.
- Delayed committing pending history until the final non-tool `Event::Done` is actually yielded. Provider errors, malformed normalized streams, and dropped streams leave `Session::history()` exactly as it was before `send`.
- Treat an empty provider stream before `RawEvent::Done` as `Error::Decode`, matching the provider seam contract that every successful provider turn terminates with exactly one `Done`.
- Preserve assistant `reasoning_content` in both committed history and follow-up provider requests after tool rounds.
- Added focused fake-provider tests for tool-round event ordering, no intermediate `Done`, argument-fragment concatenation by index, multiple sequential tool calls, unknown tools, malformed tool arguments, JSON-string tool outputs, provider-error rollback, cancellation rollback, and atomic complete-turn commits.
- After independent review, changed `finish_reason = ToolCalls` with no assembled tool calls into an `Error::Decode` rollback path and added post-tool-result rollback tests for provider errors and cancellation.
- Updated the DeepSeek placeholder provider to emit one normalized `Done` event instead of an empty stream so it remains compatible with the phase 4 core loop until real DeepSeek streaming is implemented.
- Deferred true macro-generated malformed-argument coverage to the macro/facade phases; Phase 4 uses a manual parsing tool to exercise the same `ToolOutput::Err` loop behavior without pulling macro implementation forward.

## Phase 5

- Implemented the `#[ur::tool]` attribute macro in `ur-macros`. The thin `proc_macro_attribute` entry point delegates to an internal `expand(TokenStream, TokenStream) -> syn::Result<TokenStream>` so the parsing, validation, and codegen logic is unit-testable on `proc_macro2`/`syn` types without a proc-macro context; hard errors surface through `syn::Error::into_compile_error`.
- Generated shape: a same-identifier unit struct (`#[allow(non_camel_case_types)]`, inherited visibility) plus a `const _: () = { ... }` block holding a private `__UrParams` struct (deriving `Deserialize` + `JsonSchema`) and the `Tool` impl. Keeping the impl in an anonymous const avoids leaking helper items while letting the public struct carry the forwarded attributes.
- All generated references use the absolute `::ur` root for public items and a `::ur::__rt::{serde, serde_json, schemars}` plumbing module for derives, `crate = "..."` attributes, `SchemaGenerator`, and output serialization. `ur-macros` takes no dependency on the `ur` facade; Phase 6 must wire `ur::__rt` to re-export the real `serde`, `serde_json`, and `schemars` crates.
- Schema generation uses the schemars 1.x API: `SchemaGenerator::default().into_root_schema_for::<__UrParams>().to_value()` yields the parameters `JsonValue` directly, avoiding the `schema_for!` macro path.
- Result detection is name-based on the return type's last path segment (`Result`), which covers `Result`, `core::result::Result`, and aliases like `std::io::Result`. Bare `T` is treated as infallible; both arms serialize success with `serde_json::to_string` and stringify errors via `ToString`.
- Async and sync bodies are unified by emitting the original signature/body as an inner `__ur_tool_body` fn (preserving `asyncness`) and awaiting only when the source was `async`. No-argument tools skip argument deserialization entirely so an empty/absent wire payload does not fail.
- Attribute handling: every attribute on the function is forwarded to the generated struct; `cfg`/`cfg_attr` attributes are additionally replayed on the `const _` block so the struct and its impl are gated together and never desync.
- Validation rejects, with spanned `compile_error!`s before any `::ur` path must resolve: invalid tool names (`[a-zA-Z0-9_-]{1,64}`), unknown attribute keys (anything not `description`, `name`, or a parameter name), non-`name: Type` argument patterns and `self` receivers, `impl Trait` returns, and unsupported signatures (generics, where clauses, const/unsafe/extern, variadic).
- Tests: 13 path-independent unit tests (name validation, Result detection, parameter/attribute parsing, identifier and schema-shaping assertions on the emitted token string) and 5 `trybuild` compile-fail cases with checked `.stderr`, one per validation category.
- Deferred to Phase 6 (per the plan): successful-expansion and runtime tool-call tests, schema assertions against the real `ToolSchema`, `agent.tool(add)` registration, and semantic compile-fail tests for generated trait bounds (e.g. non-`DeserializeOwned` parameters), all of which require `::ur` to resolve. A `&str`/reference parameter currently fails with a lifetime error rather than a trait-bound diagnostic; cleaner messaging is left to that phase.
- MSRV note: avoided let-chains (stabilized after 1.85) in favor of nested `if let`. The `1.85` toolchain is not installed locally, so the documented MSRV build was not re-verified for this phase.

## Phase 6

- Wired `ur::__rt` as a `#[doc(hidden)] pub mod __rt` in `ur-core` that re-exports the real `schemars`, `serde`, and `serde_json` crates, then re-exported it from the facade as `ur::__rt`. The module is unconditional (not behind the `serde` feature) because tool support always needs these crates, so macro-generated tools work in every facade feature configuration. Keeping it in `ur-core` (which already depends on all three) avoids adding those dependencies to the thin facade and keeps a single source of truth.
- The facade's public re-exports already matched `API.md`; Phase 6 added only the hidden `__rt` re-export plus dev-dependencies (`futures-util`, `serde`, `serde_json`, `tokio`, `trybuild`) for examples and tests.
- Placed the macro runtime/registration/schema tests as ordinary integration tests under `crates/ur/tests/`, where the crate under test is in scope as `ur`, so the macro's absolute `::ur` paths resolve. These cover sync and async invocation, successful output JSON serialization, error stringification, malformed-argument stringification, `Option<T>` optionality (call site and schema `required`), parameter-description folding, name override, real `ToolSchema` assertions, the `ur::BoxFuture`/`ur::ToolSchema`/`ur::ToolArguments` signature names, and an end-to-end `agent.tool(...)` round trip through the agent loop. This is stronger than a compile-only check for the runtime behavior.
- Proved visibility and doc-comment forwarding with a `#![deny(missing_docs)]` module containing a documented `pub` tool: it only compiles if the doc comment reached the generated `pub` struct. A `#[cfg(test)]`-gated tool proves the cfg is replayed onto both the struct and its impl.
- Added a compile-pass `trybuild` lock for the `API.md` `#[ur::tool]` examples plus `agent.tool(...)` registration, and an `examples/agent.rs` target that runs the full provider-agnostic flow against a scripted fake provider.
- Implemented the semantic compile-fail locks with the existing cargo-fixture harness (assert failure plus a diagnostic substring) rather than exact `trybuild` `.stderr` snapshots, since trait-bound diagnostics drift across compiler versions; this matches the plan's preference for non-brittle assertions. Covers a non-serializable return type (diagnostic mentions `Serialize`) and a parameter type that is neither deserializable nor schema-able (diagnostic mentions both `Deserialize` and `JsonSchema`).
- Locked DeepSeek feature gating: a `#[cfg(feature = "deepseek")]` test confirms `ur::deepseek::DeepSeekClient` is a `Provider`, and a no-feature compile-fail fixture confirms the `ur::deepseek` path is absent without the feature.
- Deferred to Phase 7: the complete `DEEPSEEK.md` example as a compiling target, which depends on the real `DeepSeekClient` constructors (`try_from_env`, builder) that are still placeholders. A discriminating compile-fail case that isolates a deserializable-but-non-schema parameter (or the reverse) from the combined failure is also deferred; the current fixture asserts both diagnostics for a type satisfying neither bound.
- MSRV note: the `1.85` toolchain is not installed locally, so the documented MSRV build was not re-verified for this phase.
