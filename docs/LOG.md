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
