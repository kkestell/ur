//! Workspace-level integration tests for the facade, macro-generated tools, core
//! agent loop, and a scripted provider.

mod support;

use futures_util::StreamExt as _;
use serde::Serialize;
use support::FakeProvider;
use ur::{Agent, Event, FinishReason, MessageRole, Model, RawEvent, ToolOutput};

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
async fn weather(city: String) -> Result<Weather, std::io::Error> {
    Ok(Weather {
        temp_c: 18.5,
        summary: format!("clear skies over {city}"),
    })
}

fn done(finish_reason: FinishReason) -> RawEvent {
    RawEvent::Done {
        finish_reason,
        usage: None,
    }
}

#[tokio::test]
async fn provider_agnostic_api_example_runs_through_facade_and_core() {
    let provider = FakeProvider::new([
        vec![
            RawEvent::ReasoningDelta("need tools".to_owned()),
            RawEvent::ToolCallDelta {
                index: 0,
                id: Some("call-add".to_owned()),
                name: Some("add".to_owned()),
                arguments: r#"{"a":41,"b":1}"#.to_owned(),
            },
            RawEvent::ToolCallDelta {
                index: 1,
                id: Some("call-weather".to_owned()),
                name: Some("weather".to_owned()),
                arguments: r#"{"city":"Paris"}"#.to_owned(),
            },
            done(FinishReason::ToolCalls),
        ],
        vec![
            RawEvent::TextDelta("41 + 1 is 42, and Paris is clear.".to_owned()),
            done(FinishReason::Stop),
        ],
    ]);
    let model = Model::new(provider, "fake-model");
    let agent = Agent::new("You are a concise assistant. Use tools when useful.", model)
        .tool(add)
        .tool(weather);
    let mut session = agent.session();

    let mut text = String::new();
    let mut tool_outputs = Vec::new();
    let mut events = session.send("What is 41 + 1, and what is the Paris weather?");
    while let Some(event) = events.next().await {
        match event.expect("scripted provider succeeds") {
            Event::TextDelta { delta } => text.push_str(&delta),
            Event::ToolResult { output, .. } => tool_outputs.push(output),
            _ => {}
        }
    }
    drop(events);

    assert_eq!(text, "41 + 1 is 42, and Paris is clear.");
    assert_eq!(tool_outputs.len(), 2);
    assert_eq!(tool_outputs[0], ToolOutput::Ok("42".to_owned()));
    let weather_json = match &tool_outputs[1] {
        ToolOutput::Ok(content) => serde_json::from_str::<serde_json::Value>(content).unwrap(),
        other => panic!("expected weather success, got {other:?}"),
    };
    assert_eq!(weather_json["summary"], "clear skies over Paris");

    let history = session.history();
    assert_eq!(history.len(), 6);
    assert_eq!(history[0].role(), MessageRole::System);
    assert_eq!(history[1].role(), MessageRole::User);
    assert_eq!(history[2].role(), MessageRole::Assistant);
    assert_eq!(history[2].reasoning_content(), Some("need tools"));
    assert_eq!(history[2].tool_calls().len(), 2);
    assert_eq!(history[3].tool_call_id(), Some("call-add"));
    assert_eq!(history[3].content(), Some("42"));
    assert_eq!(history[4].tool_call_id(), Some("call-weather"));
    assert_eq!(
        history[5].content(),
        Some("41 + 1 is 42, and Paris is clear.")
    );
}
