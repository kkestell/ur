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
