# ur

A Rust library for async agents — owns the full loop (streaming, reasoning, tool dispatch, multi-turn history, rollback on failure) over a single pluggable `Provider` trait.

```rust
use futures_util::StreamExt;

#[ur::tool(description = "Add two integers.")]
async fn add(a: i64, b: i64) -> i64 { a + b }

#[tokio::main]
async fn main() -> ur::Result<()> {
    let client = ur::openai::OpenAiClient::try_from_env()?;
    let model = ur::Model::new(client, "gpt-5.5");

    let agent = ur::Agent::new("You are a concise assistant. Use tools when useful.", model)
        .tool(add);

    let mut session = agent.session();
    let mut events = session.send("What is 41 + 1? Use the tool.");
    while let Some(event) = events.next().await {
        match event? {
            ur::Event::TextDelta { delta } => print!("{delta}"),
            ur::Event::Done { .. } => break,
            _ => {}
        }
    }
    Ok(())
}
```

For tools that need state — a database handle, an HTTP client, a cancel token — put `&self` methods in an `#[ur::tools]` impl block and register the whole set with `agent.tool_set(...)`. The state type must be `Clone + Send + Sync + 'static`, which is cheap when its fields are `Arc<_>` or already-`Clone` handles. Each `#[ur::tool]` method becomes a tool backed by a clone of the state; unmarked methods are untouched.

```rust
use std::sync::Arc;

#[derive(Clone)]
struct Tools {
    db: Arc<Db>,
    http: reqwest::Client,
}

#[ur::tools]
impl Tools {
    #[ur::tool(description = "Look up a user by id.")]
    async fn get_user(&self, id: u64) -> Result<User, String> {
        self.db.fetch(id).await.map_err(|e| e.to_string())
    }

    #[ur::tool(description = "Fetch a URL and return its body.")]
    async fn fetch(&self, url: String) -> Result<String, String> {
        let resp = self.http.get(&url).send().await.map_err(|e| e.to_string())?;
        resp.text().await.map_err(|e| e.to_string())
    }
}

let agent = ur::Agent::new("You are a helpful assistant.", model)
    .tool(add)              // stateless free-fn tool
    .tool_set(Tools { db, http });   // both methods, sharing one `Tools`
```

## Features

- **Provider-agnostic agent loop.** `Model`, `Agent`, `Session`, and `EventStream` work identically with any `Provider` implementation.
- **Streaming deltas.** `TextDelta`, `ReasoningDelta`, and incremental `ToolCall` assembly as events arrive.
- **Tool dispatch with rollback.** Tools run sequentially in call order. A provider error or dropped stream rolls the session back to its last committed state.
- **Cancellable turns.** Drop the `EventStream` to cancel, or call `stream.abort_handle()` to obtain a cheap, clonable `AbortHandle` that cancels the turn from another task or thread. Either way in-flight provider and tool work is abandoned and the turn rolls back.
- **`#[ur::tool]` / `#[ur::tools]` macros.** Annotate a free `async fn` and register it with `agent.tool(add)`, or put `&self` methods on an `#[ur::tools]` impl block for stateful tools and register them with `agent.tool_set(...)`. Parameters and return types derive JSON Schema automatically.
- **Structured outputs.** A `json_schema` response format constrains a reply to a schema, derived from a Rust type with `ResponseFormat::json_schema_for::<T>` or hand-built.
- **Pluggable providers.** Implement `Provider::chat` and `Provider::model_spec` to drive any backend. OpenAI, DeepSeek, and OpenRouter ship in the workspace; additional providers live in their own crates.

## Quick start

Add the crate to your `Cargo.toml`. It is published as `ur-rs` and imported as `ur`:

```toml
[dependencies]
ur = { package = "ur-rs", version = "0.1" }
tokio = { version = "1", features = ["full"] }
futures-util = "0.3"
```

Set `OPENAI_API_KEY` in your environment (or pass the key explicitly to `OpenAiClient::new`), then run the example above.

## Crates

| Crate              | Role                                                                                         |
| ------------------ | -------------------------------------------------------------------------------------------- |
| `ur-rs`            | Facade (imported as `ur`): re-exports `ur-core` and enabled provider crates.                 |
| `ur-core`          | Provider-agnostic types: `Agent`, `Model`, `Session`, events, the `Provider` trait, `Error`. |
| `ur-macros`        | The `#[ur::tool]` and `#[ur::tools]` proc-macros.                                            |
| `ur-openai-compat` | Shared plumbing for the OpenAI-compatible providers (request/SSE/retry machinery).           |
| `ur-openai`        | OpenAI `Provider` implementation.                                                            |
| `ur-deepseek`      | DeepSeek `Provider` implementation.                                                          |
| `ur-openrouter`    | OpenRouter `Provider` implementation.                                                        |

## Providers

Implement `Provider` to drive any LLM backend:

```rust
use ur::{BoxStream, ModelSpec, Provider, RawEvent, Request, Result};

struct MyProvider;

impl Provider for MyProvider {
    fn chat(&self, request: &Request) -> BoxStream<'static, Result<RawEvent>> {
        // Map your backend's streaming response into normalized RawEvents.
        todo!()
    }

    fn model_spec(&self, model_id: &str) -> Option<ModelSpec> {
        // Return catalog facts for known model ids.
        None
    }
}
```

See [`docs/providers/openai.md`](docs/providers/openai.md) for the default provider, [`docs/providers/deepseek.md`](docs/providers/deepseek.md) for the DeepSeek provider, and [`docs/providers/openrouter.md`](docs/providers/openrouter.md) for the OpenRouter provider.

## Settings

Generation settings are configured on `Model` before creating an `Agent`:

```rust
let model = ur::Model::new(provider, "gpt-5.5")
    .thinking(ur::Thinking::Enabled)
    .reasoning_effort(ur::ReasoningEffort::High)
    .max_tokens(4096)
    .temperature(0.7)
    .top_p(0.9)
    .stop(["END".to_owned()])
    .response_format(ur::ResponseFormat::JsonObject);
```

`ResponseFormat` also has `JsonSchema` for structured outputs — build it with `ResponseFormat::json_schema_for::<T>(name)` to derive the schema from a Rust type, or `ResponseFormat::json_schema(name, schema)` for a hand-built schema.

## Examples

Runnable examples live in [`crates/ur/examples`](crates/ur/examples). Run one with `cargo run`:

```sh
# Implements a custom Provider; runs offline, no API key.
cargo run -p ur-rs --example custom

# OpenAI examples (default features); need OPENAI_API_KEY.
cargo run -p ur-rs --example minimal

# DeepSeek examples; need the `deepseek` feature and DEEPSEEK_API_KEY.
cargo run -p ur-rs --example thinking --features deepseek

# OpenRouter examples; need the `openrouter` feature and OPENROUTER_API_KEY.
cargo run -p ur-rs --example openrouter --features openrouter
```

| Example                 | Provider     | Shows                                                       |
| ----------------------- | ------------ | ----------------------------------------------------------- |
| `custom`                | none (local) | Implementing a custom `Provider`; runs offline, no key.     |
| `minimal`               | OpenAI       | The smallest send-and-stream program.                       |
| `openai`                | OpenAI       | The complete OpenAI flow with tool calls.                   |
| `stateful`              | OpenAI       | Stateful tools via `#[ur::tools]` and `tool_set`.           |
| `builder`               | OpenAI       | Configuring `OpenAiClient` through its builder.             |
| `session`               | OpenAI       | A multi-turn conversation with retained history.            |
| `json`                  | OpenAI       | Requesting a JSON-object response.                          |
| `effort`                | OpenAI       | Tuning `ReasoningEffort`.                                   |
| `strict`                | OpenAI       | A hand-written strict-mode tool schema.                     |
| `structured_openai`     | OpenAI       | A `json_schema` response format derived from a Rust type.   |
| `deepseek`              | DeepSeek     | The complete DeepSeek flow with tool calls.                 |
| `thinking`              | DeepSeek     | Toggling `Thinking` mode.                                   |
| `openrouter`            | OpenRouter   | The complete OpenRouter flow with tool calls.               |
| `structured_openrouter` | OpenRouter   | A `json_schema` response format over OpenRouter.            |

Every example except `custom` calls a live API and requires the matching API key in the environment. The DeepSeek examples also require `--features deepseek`, and the OpenRouter examples require `--features openrouter`.

## Minimum supported Rust version

MSRV is Rust 1.88.

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.
