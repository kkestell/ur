# TODO — missing features / functionality

Working notes on gaps in the agent library, ordered by how glaring they are. Local-only scratch file (excluded via `.git/info/exclude`, not `.gitignore`).

## Tier 1 — most obvious

- [ ] **Multimodal input (images).** `UserMessage` is now an ordered `Vec<ContentPart>` and `Message` carries `content_parts`; images attach by URL or raw bytes (`crates/ur-core/src/content.rs`). The OpenAI encoder emits a `content` array with `image_url` parts; DeepSeek rejects images. Audio / file input parts are still open — `ContentPart` is `#[non_exhaustive]` to allow them later without a breaking change.

- [ ] **Bound the agent loop.** The loop re-enters `start_provider_turn()` for as long as the model keeps emitting tool calls (`crates/ur-core/src/event.rs:294`) with no iteration counter — unbounded API spend on a looping model. Add a `max_steps` cap that surfaces a terminal event when hit. Cheapest real safety/cost win; do this first.

- [ ] **`tool_choice` control.** Hardcoded to `"auto"` (`crates/ur-openai/src/request.rs:26`); no field on `Settings` (`crates/ur-core/src/provider.rs:236`). Can't force a tool, require _some_ tool, or disable tools for a turn.

- [x] **Structured outputs (JSON-schema response format).** `ResponseFormat::JsonSchema(JsonSchemaFormat)` carries a schema built by name from a Rust type (`ResponseFormat::json_schema_for::<T>`) or a raw value (`crates/ur-core/src/model.rs`). OpenAI encodes it natively as `response_format: json_schema`, reusing the strict-schema rewriter (`crates/ur-openai/src/request.rs`); strict defaults to on. DeepSeek has no native equivalent and rejects it with `Error::Config` for now — see the emulation item below.

## Tier 2 — expected

- [ ] **DeepSeek structured outputs (emulation).** DeepSeek's `response_format` only accepts `text` / `json_object`, so `ResponseFormat::JsonSchema` currently errors there (`crates/ur-deepseek/src/request.rs`). DeepSeek _does_ support constrained decoding for tool-call arguments via beta strict tools, so structured output can be emulated: emit the schema as a single forced strict function tool on the beta endpoint and remap the returned tool-call arguments back into assistant text inside the DeepSeek crate (SSE translation), leaving the core agent loop untouched. Gives enforcement identical to OpenAI's native path.

- [ ] **Parallel tool execution.** Tools run strictly sequentially (`crates/ur-core/src/event.rs:294`); models emit parallel calls expecting concurrency. Futures are already boxed — `join_all` over the pending queue.

- [ ] **History management.** `Model::context_window()` is exposed (`crates/ur-core/src/lib.rs:149`) but unused. No truncation / compaction / summarization — long sessions grow until the provider rejects the request.

- [ ] **Mutable / seedable session history.** `Session` offers only `send` / `reset` / `history` (`crates/ur-core/src/lib.rs:382`). No way to seed few-shot history, inject a synthetic tool result, edit, or resume from saved state — despite `Message` being `serde`-serializable. Add `Session::from_history` / `with_history`.

- [ ] **Stateful macro tools.** `#[ur::tool]` only wraps free `async fn`s, so a tool can't carry a DB handle / HTTP client / cancel token. Manual `Tool` impl with `Arc<State>` works, but the ergonomic path is stateless.

## Tier 3 — minor / latent

- [ ] **Explicit cancellation.** Currently drop-only (does roll back correctly, `crates/ur-core/src/event.rs:233`); no abort handle.
- [ ] **`ReasoningEffort::ExtraHigh` / `Max` collapse to `"high"`** (`crates/ur-openai/src/request.rs:111`) — three enum variants, one wire value. Latent bug or undocumented limitation.
- [ ] **More sampling params** — `seed`, `frequency_penalty`, `presence_penalty`, `logit_bias`.

## Already covered (not gaps)

- Retries + timeouts live in the OpenAI client (`crates/ur-openai/src/client.rs:12`, default 3 retries / 10 min).
- `base_url` override (`crates/ur-openai/src/client.rs:112`) already targets any OpenAI-compatible endpoint (Ollama, vLLM, OpenRouter), so provider breadth is less pressing than it looks.
