## Phase 1

- Created a Cargo workspace with `ur`, `ur-core`, `ur-macros`, and `ur-deepseek` crates under `crates/`.
- Set shared package metadata to Kyle Kestell <kyle@kestell.org>, repository/homepage `https://github.com/kkestell/ur`, edition 2024, MSRV 1.85, and `MIT OR Apache-2.0`.
- Kept `ur-core` runtime-agnostic: its normal dependency tree has no `tokio` or `reqwest`. Runtime and HTTP dependencies start in `ur-deepseek`.
- Wired the `ur` facade with default features `serde` and `deepseek`; `cargo test -p ur --no-default-features` verifies provider-free facade builds still compile.
- Kept `serde`, `serde_json`, and `schemars` as unconditional `ur-core` dependencies, matching `API.md`: tool support needs them, while the facade `serde` feature controls public `Serialize`/`Deserialize` impls.
- Deferred `proc-macro2`, `quote`, and `syn` until the macro implementation needs parsing and code generation.
- Added placeholder public items only where needed to prove crate/module boundaries and re-export paths. Full semantics remain deferred to later phases.
- Committed `Cargo.lock` for repeatable workspace validation. Cargo selected dependency versions compatible with Rust 1.85.
