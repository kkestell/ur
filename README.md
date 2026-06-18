# ur

Async tool-using LLM agents over a pluggable provider backend.

`ur` owns the full agent loop — streaming, reasoning, tool dispatch, multi-turn history, and rollback — over a single `Provider` trait. Providers ship as separate crates, enabled by Cargo features. The OpenAI provider is included by default.

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

## Features

- **Provider-agnostic agent loop.** `Model`, `Agent`, `Session`, and `EventStream` work identically with any `Provider` implementation.
- **Streaming deltas.** `TextDelta`, `ReasoningDelta`, and incremental `ToolCall` assembly as events arrive.
- **Tool dispatch with rollback.** Tools run sequentially in call order. A provider error or dropped stream rolls the session back to its last committed state.
- **`#[ur::tool]` macro.** Annotate an `async fn` and register it with `agent.tool(add)`. Parameters and return types derive JSON Schema automatically.
- **Pluggable providers.** Implement `Provider::chat` and `Provider::model_spec` to drive any backend. OpenAI and DeepSeek ship in the workspace; additional providers live in their own crates.

## Quick start

Add `ur` to your `Cargo.toml`:

```toml
[dependencies]
ur = "0.1"
tokio = { version = "1", features = ["full"] }
futures-util = "0.3"
```

Set `OPENAI_API_KEY` in your environment (or pass the key explicitly to `OpenAiClient::new`), then run the example above.

## Crates

| Crate         | Role                                                                                         |
| ------------- | -------------------------------------------------------------------------------------------- |
| `ur`          | Facade: re-exports `ur-core` and enabled provider crates.                                    |
| `ur-core`     | Provider-agnostic types: `Agent`, `Model`, `Session`, events, the `Provider` trait, `Error`. |
| `ur-macros`   | The `#[ur::tool]` proc-macro.                                                                |
| `ur-openai`   | OpenAI `Provider` implementation.                                                            |
| `ur-deepseek` | DeepSeek `Provider` implementation.                                                          |

## Provider seam

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

See [`docs/providers/openai.md`](docs/providers/openai.md) for the default provider and [`docs/providers/deepseek.md`](docs/providers/deepseek.md) for the DeepSeek provider.

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

## Minimum supported Rust version

MSRV is Rust 1.88.

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.
