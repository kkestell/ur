#![cfg(feature = "openai")]

use std::fs;
use std::path::Path;

use futures_util::StreamExt as _;
use serde_json::{Value, json};
use ur::{Event, FinishReason, Model, ResponseFormat, ToolOutput};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[ur::tool(description = "Add two integers.")]
async fn add(a: i64, b: i64) -> i64 {
    a + b
}

#[derive(Debug, PartialEq, serde::Deserialize, schemars::JsonSchema)]
struct Capital {
    city: String,
    country: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct Geo {
    latitude: f64,
    longitude: f64,
}

// A nested struct: `schemars` emits `Geo` under `$defs` and references it with
// `$ref`, so this only encodes (and decodes) correctly once the strict rewriter
// closes the referenced definition too.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct LocatedCapital {
    city: String,
    country: String,
    location: Geo,
}

fn sse(data: &str) -> ResponseTemplate {
    ResponseTemplate::new(200)
        .insert_header("content-type", "text/event-stream")
        .set_body_string(data)
}

fn chunk(value: Value) -> String {
    format!("data: {value}\n\n")
}

fn done(finish_reason: &str) -> String {
    chunk(json!({
        "choices": [{
            "delta": {},
            "finish_reason": finish_reason
        }],
        "usage": null
    }))
}

fn openai_client(server: &MockServer) -> ur::openai::OpenAiClient {
    ur::openai::OpenAiClient::builder()
        .api_key("test-key")
        .base_url(server.uri())
        .max_retries(0)
        .build()
        .unwrap()
}

#[tokio::test]
async fn mocked_openai_tool_round_trips_through_session_send() {
    let server = MockServer::start().await;
    let tool_call_body = format!(
        "{}{}{}data: [DONE]\n\n",
        chunk(json!({
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "id": "call-1",
                        "type": "function",
                        "function": {
                            "name": "add",
                            "arguments": "{\"a\":41"
                        }
                    }]
                },
                "finish_reason": null
            }],
            "usage": null
        })),
        chunk(json!({
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "function": {
                            "arguments": ",\"b\":1}"
                        }
                    }]
                },
                "finish_reason": null
            }],
            "usage": null
        })),
        done("tool_calls"),
    );
    let final_body = format!(
        "{}{}data: [DONE]\n\n",
        chunk(json!({
            "choices": [{
                "delta": { "content": "The answer is 42." },
                "finish_reason": null
            }],
            "usage": null
        })),
        done("stop"),
    );

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(header("authorization", "Bearer test-key"))
        .respond_with(sse(&tool_call_body))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(header("authorization", "Bearer test-key"))
        .respond_with(sse(&final_body))
        .expect(1)
        .mount(&server)
        .await;

    let client = openai_client(&server);
    let model = Model::new(client, "gpt-5.5");
    let agent = ur::Agent::new("Use tools when useful.", model).tool(add);
    let mut session = agent.session();

    let mut text = String::new();
    let mut saw_tool_call = false;
    let mut saw_done = false;
    let mut events = session.send("What is 41 + 1? Use the tool.");
    while let Some(event) = events.next().await {
        match event.expect("mocked OpenAI stream succeeds") {
            Event::TextDelta { delta } => text.push_str(&delta),
            Event::ToolCall {
                id,
                name,
                arguments,
            } => {
                assert_eq!(id, "call-1");
                assert_eq!(name, "add");
                assert_eq!(arguments.as_str(), r#"{"a":41,"b":1}"#);
                saw_tool_call = true;
            }
            Event::ToolResult { output, .. } => {
                assert_eq!(output, ToolOutput::Ok("42".to_owned()));
            }
            Event::Done { finish_reason } => {
                assert_eq!(finish_reason, FinishReason::Stop);
                saw_done = true;
            }
            _ => {}
        }
    }
    drop(events);

    assert!(saw_tool_call);
    assert!(saw_done);
    assert_eq!(text, "The answer is 42.");

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2);

    let first: Value = requests[0].body_json().unwrap();
    assert_eq!(first["model"], "gpt-5.5");
    assert_eq!(first["messages"][0]["role"], "system");
    assert_eq!(first["messages"][1]["role"], "user");
    assert_eq!(first["tools"][0]["function"]["name"], "add");
    assert_eq!(first["tool_choice"], "auto");
    assert_eq!(first["stream"], true);
    assert_eq!(first["stream_options"]["include_usage"], true);

    let second: Value = requests[1].body_json().unwrap();
    assert_eq!(second["messages"][2]["role"], "assistant");
    assert_eq!(
        second["messages"][2]["tool_calls"][0]["function"]["arguments"],
        r#"{"a":41,"b":1}"#
    );
    assert_eq!(second["messages"][3]["role"], "tool");
    assert_eq!(second["messages"][3]["tool_call_id"], "call-1");
    assert_eq!(second["messages"][3]["content"], "42");
}

#[tokio::test]
async fn mocked_openai_json_schema_request_is_well_formed() {
    let server = MockServer::start().await;
    let body = format!(
        "{}{}data: [DONE]\n\n",
        chunk(json!({
            "choices": [{
                "delta": { "content": "{\"city\":\"Paris\",\"country\":\"France\"}" },
                "finish_reason": null
            }],
            "usage": null
        })),
        done("stop"),
    );
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(header("authorization", "Bearer test-key"))
        .respond_with(sse(&body))
        .expect(1)
        .mount(&server)
        .await;

    let client = openai_client(&server);
    let model = Model::new(client, "gpt-5.5")
        .response_format(ResponseFormat::json_schema_for::<Capital>("capital"));
    let agent = ur::Agent::new("Answer with the requested structured data.", model);
    let mut session = agent.session();

    let mut text = String::new();
    let mut events = session.send("What is the capital of France?");
    while let Some(event) = events.next().await {
        if let Event::TextDelta { delta } = event.expect("mocked OpenAI stream succeeds") {
            text.push_str(&delta);
        }
    }
    drop(events);

    assert_eq!(
        serde_json::from_str::<Capital>(&text).unwrap(),
        Capital {
            city: "Paris".to_owned(),
            country: "France".to_owned(),
        }
    );

    let requests = server.received_requests().await.unwrap();
    let request: Value = requests[0].body_json().unwrap();
    let format = &request["response_format"];
    assert_eq!(format["type"], "json_schema");

    let json_schema = &format["json_schema"];
    assert_eq!(json_schema["name"], "capital");
    assert_eq!(json_schema["strict"], true);

    let schema = &json_schema["schema"];
    assert_eq!(schema["type"], "object");
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(schema["required"], json!(["city", "country"]));
}

#[tokio::test]
async fn mocked_openai_nested_json_schema_closes_definitions() {
    let server = MockServer::start().await;
    let body = format!(
        "{}{}data: [DONE]\n\n",
        chunk(json!({
            "choices": [{ "delta": { "content": "{}" }, "finish_reason": null }],
            "usage": null
        })),
        done("stop"),
    );
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(header("authorization", "Bearer test-key"))
        .respond_with(sse(&body))
        .expect(1)
        .mount(&server)
        .await;

    let client = openai_client(&server);
    let model = Model::new(client, "gpt-5.5").response_format(ResponseFormat::json_schema_for::<
        LocatedCapital,
    >("located_capital"));
    let agent = ur::Agent::new("Answer with the requested structured data.", model);
    let mut session = agent.session();

    let mut events = session.send("Where is the capital of France?");
    while let Some(event) = events.next().await {
        let _ = event.expect("mocked OpenAI stream succeeds");
    }
    drop(events);

    let requests = server.received_requests().await.unwrap();
    let request: Value = requests[0].body_json().unwrap();
    let schema = &request["response_format"]["json_schema"]["schema"];

    // The referenced definition is constrained just like the root object.
    let geo = &schema["$defs"]["Geo"];
    assert_eq!(geo["additionalProperties"], false);
    assert_eq!(geo["required"], json!(["latitude", "longitude"]));
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(schema["required"], json!(["city", "country", "location"]));
}

#[tokio::test]
#[ignore = "live OpenAI smoke test; requires OPENAI_API_KEY in the environment or .env"]
async fn live_openai_nested_structured_output_smoke_test() {
    let client = live_client();
    let model = Model::new(client, "gpt-5.5")
        .max_tokens(256)
        .response_format(ResponseFormat::json_schema_for::<LocatedCapital>(
            "located_capital",
        ));
    let agent = ur::Agent::new("Answer with the requested structured data.", model);
    let mut session = agent.session();

    let mut text = String::new();
    let mut events = session.send("What is the capital of France and its coordinates?");
    while let Some(event) = events.next().await {
        if let Event::TextDelta { delta } = event.expect("live nested structured request succeeds")
        {
            text.push_str(&delta);
        }
    }
    drop(events);

    let located: LocatedCapital =
        serde_json::from_str(&text).expect("nested structured output parses into the schema type");
    assert_eq!(located.country, "France");
    assert!(!located.city.is_empty());
    assert!(located.location.latitude.is_finite());
    assert!(located.location.longitude.is_finite());
}

#[tokio::test]
#[ignore = "live OpenAI smoke test; requires OPENAI_API_KEY in the environment or .env"]
async fn live_openai_structured_output_smoke_test() {
    let client = live_client();
    let model = Model::new(client, "gpt-5.5")
        .max_tokens(256)
        .response_format(ResponseFormat::json_schema_for::<Capital>("capital"));
    let agent = ur::Agent::new("Answer with the requested structured data.", model);
    let mut session = agent.session();

    let mut text = String::new();
    let mut events = session.send("What is the capital of France?");
    while let Some(event) = events.next().await {
        if let Event::TextDelta { delta } = event.expect("live structured request succeeds") {
            text.push_str(&delta);
        }
    }
    drop(events);

    let capital: Capital =
        serde_json::from_str(&text).expect("structured output parses into the schema type");
    assert_eq!(capital.country, "France");
    assert!(!capital.city.is_empty());
}

#[tokio::test]
#[ignore = "live OpenAI smoke test; requires OPENAI_API_KEY in the environment or .env"]
async fn live_openai_text_only_smoke_test() {
    let client = live_client();
    let model = Model::new(client, "gpt-5.5")
        .reasoning_effort(ur::ReasoningEffort::Low)
        .max_tokens(256);
    let agent = ur::Agent::new("Answer with one short sentence.", model);
    let mut session = agent.session();

    let mut saw_text = false;
    let mut saw_done = false;
    let mut events = session.send("Say hello from ur.");
    while let Some(event) = events.next().await {
        match event.expect("live text-only request succeeds") {
            Event::TextDelta { delta } => saw_text |= !delta.is_empty(),
            Event::Done { .. } => saw_done = true,
            _ => {}
        }
    }

    assert!(saw_text);
    assert!(saw_done);
}

#[tokio::test]
#[ignore = "live OpenAI smoke test; requires OPENAI_API_KEY in the environment or .env"]
async fn live_openai_tool_call_smoke_test() {
    let client = live_client();
    let model = Model::new(client, "gpt-5.5").max_tokens(128);
    let agent = ur::Agent::new(
        "Use the add tool to answer arithmetic questions. Keep the final answer short.",
        model,
    )
    .tool(add);
    let mut session = agent.session();

    let mut saw_tool_result = false;
    let mut saw_done = false;
    let mut events = session.send("What is 41 + 1? Use the add tool.");
    while let Some(event) = events.next().await {
        match event.expect("live tool request succeeds") {
            Event::ToolResult { output, .. } => {
                assert_eq!(output, ToolOutput::Ok("42".to_owned()));
                saw_tool_result = true;
            }
            Event::Done { .. } => saw_done = true,
            _ => {}
        }
    }

    assert!(saw_tool_result);
    assert!(saw_done);
}

fn live_client() -> ur::openai::OpenAiClient {
    let api_key = std::env::var("OPENAI_API_KEY")
        .ok()
        .or_else(|| api_key_from_dotenv(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.env")))
        .expect("OPENAI_API_KEY must be set in the environment or .env");

    ur::openai::OpenAiClient::new(api_key)
}

fn api_key_from_dotenv(path: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(path).ok().and_then(|contents| {
        contents.lines().find_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }

            let (key, value) = line.split_once('=')?;
            (key.trim() == "OPENAI_API_KEY")
                .then(|| value.trim().trim_matches('"').trim_matches('\'').to_owned())
        })
    })
}
