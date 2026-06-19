# AGENTS.md

THIS FILE MUST BE KEPT UP TO DATE AT ALL TIMES

`ur` is a Rust library for async, tool-using LLM agents built over a pluggable provider backend. It owns the full agent loop — streaming, reasoning, tool dispatch, multi-turn history, and rollback — behind a single `Provider` trait, with providers (OpenAI by default; DeepSeek and OpenRouter optional) shipping as separate feature-gated crates.

## Tech Stack

- **Language:** Rust (edition 2024, MSRV 1.88)
- **Build system:** Cargo (workspace, resolver 3)
- **Key dependencies:** `tokio` + `futures-*` (async/streaming), `reqwest` (rustls TLS, streaming HTTP), `serde`/`serde_json` + `schemars` (JSON + schema derivation), `thiserror`, `tracing`; `wiremock` and `trybuild` for tests.

## Codebase Map

This is a Cargo workspace of seven crates plus docs.

- `crates/ur-core/` — Provider-agnostic core: `Agent`, `Model`, `Session`, the event stream, the `Provider` trait, tool plumbing (`tool.rs`), the shared strict-mode JSON Schema rewriter (`schema.rs`, used by every provider for strict tools and `json_schema` response formats), and `Error`.
- `crates/ur-macros/` — The `#[ur::tool]` proc-macro that turns an `async fn` into a registrable tool with a derived JSON Schema. Has `trybuild` UI tests under `tests/ui`.
- `crates/ur-openai-compat/` — Shared plumbing for the OpenAI-compatible providers: API-key/user validation (`keys.rs`), request-body encode helpers (`request.rs`), SSE line framing + completion folding + generic wire structs (`sse.rs`), and the HTTP retry/stream state machine (`executor.rs`). Each provider supplies its deltas via the `executor::Dialect` trait and its own `decode_chunk`.
- `crates/ur-openai/` — OpenAI `Provider` implementation over `ur-openai-compat`: client/config, request mapping, the `decode_chunk`/wire shapes, and a `Dialect` impl (retry table, error mapping).
- `crates/ur-deepseek/` — DeepSeek `Provider` implementation over `ur-openai-compat`, plus a model `catalog`; keeps its divergent pieces local (reasoning_content, all-or-nothing strict tools, catalog max_tokens, thinking-gated sampling, untyped error-body extraction).
- `crates/ur-openrouter/` — OpenRouter `Provider` implementation over `ur-openai-compat` (OpenAI-compatible aggregator), adding app-attribution headers (`HTTP-Referer`/`X-Title`) via the `Dialect`'s `decorate` hook, a `reasoning` object, and `ProviderRouting`.
- `crates/ur/` — Public facade crate, published to crates.io as `ur-rs` but imported as `ur` (via `[lib] name = "ur"`). Re-exports `ur-core`, `ur-macros`, and feature-enabled providers. Holds the runnable `examples/` and the workspace integration/compile tests under `tests/`.
- `docs/providers/` — Per-provider reference docs (`openai.md`, `deepseek.md`, `openrouter.md`).

## Commands

Standard Cargo across the workspace. The facade (package `ur-rs`) defaults to the `openai` feature; enable `deepseek` or `openrouter` explicitly when needed.

- Build: `cargo build` (whole workspace) — provider-feature combos: `cargo build -p ur-rs --features deepseek`, `cargo build -p ur-rs --features openrouter`
- Test: `cargo test`
- Run an example: `cargo run -p ur-rs --example <name>` (e.g. `agent` runs offline; OpenAI examples need `OPENAI_API_KEY`; DeepSeek examples need `--features deepseek` and `DEEPSEEK_API_KEY`; the `openrouter` example needs `--features openrouter` and `OPENROUTER_API_KEY`)
- Lint: `cargo clippy --all-targets`
- Format: `cargo fmt`
