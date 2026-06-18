//! The provider-agnostic flow from the `ur` API documentation, driven by a small
//! scripted fake provider so the example runs without network access. Replace the
//! provider and model id with a real provider crate (for example
//! `ur::deepseek::DeepSeekClient`) to talk to a live model.

use std::collections::VecDeque;
use std::sync::Mutex;

use futures_util::StreamExt;
use serde::Serialize;
use ur::{BoxStream, Model, Provider, RawEvent, Request, Result};

#[ur::tool(description = "Add two integers.")]
async fn add(a: i64, b: i64) -> i64 {
    a + b
}

#[derive(Serialize)]
struct Weather {
    temp_c: f64,
    summary: String,
}

#[ur::tool(description = "Look up the current weather for a city.")]
async fn weather(city: String) -> std::result::Result<Weather, std::io::Error> {
    Ok(Weather {
        temp_c: 18.5,
        summary: format!("clear skies over {city}"),
    })
}

/// A provider that replays a fixed script: one tool call, then a final answer.
struct ScriptedProvider {
    batches: Mutex<VecDeque<Vec<RawEvent>>>,
}

impl ScriptedProvider {
    fn new() -> Self {
        let batches = VecDeque::from([
            vec![
                RawEvent::ToolCallDelta {
                    index: 0,
                    id: Some("call-1".to_owned()),
                    name: Some("add".to_owned()),
                    arguments: r#"{"a":41,"b":1}"#.to_owned(),
                },
                RawEvent::Done {
                    finish_reason: ur::FinishReason::ToolCalls,
                    usage: None,
                },
            ],
            vec![
                RawEvent::TextDelta("41 + 1 = 42.".to_owned()),
                RawEvent::Done {
                    finish_reason: ur::FinishReason::Stop,
                    usage: None,
                },
            ],
        ]);
        Self {
            batches: Mutex::new(batches),
        }
    }
}

impl Provider for ScriptedProvider {
    fn chat(&self, _request: &Request) -> BoxStream<'static, Result<RawEvent>> {
        let batch = self.batches.lock().unwrap().pop_front().unwrap_or_default();
        Box::pin(futures_util::stream::iter(batch.into_iter().map(Ok)))
    }

    fn model_spec(&self, _model_id: &str) -> Option<ur::ModelSpec> {
        None
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ur::Result<()> {
    let provider = ScriptedProvider::new();
    let model = Model::new(provider, "scripted-model");

    let agent = ur::Agent::new("You are a concise assistant. Use tools when useful.", model)
        .tool(add)
        .tool(weather);

    let mut session = agent.session();
    let mut events = session.send("What is 41 + 1? Use the tool.");
    while let Some(event) = events.next().await {
        match event? {
            ur::Event::TextDelta { delta } => print!("{delta}"),
            ur::Event::ReasoningDelta { .. } => {}
            ur::Event::ToolCall {
                name, arguments, ..
            } => eprintln!("\ncall {name}({arguments})"),
            ur::Event::ToolResult { output, .. } => match output {
                ur::ToolOutput::Ok(v) => eprintln!("result: {v}"),
                ur::ToolOutput::Err(e) => eprintln!("error: {e}"),
            },
            ur::Event::Usage { usage } => eprintln!(
                "tokens: in={} (cached {}) out={}",
                usage.prompt_tokens,
                usage.cached_prompt_tokens.unwrap_or(0),
                usage.completion_tokens,
            ),
            ur::Event::Done { .. } => break,
            _ => {}
        }
    }
    println!();
    Ok(())
}
