//! Facade-level tests for `#[ur::tool]`: these exercise the generated code that
//! requires `::ur` to resolve — registration, invocation, schema generation, and
//! attribute preservation.

mod support;

use futures_util::StreamExt;
use serde::Serialize;
use support::FakeProvider;
use ur::{Agent, Event, FinishReason, Model, RawEvent, Tool, ToolArguments, ToolOutput};

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

/// A synchronous tool: the macro wraps a non-`async fn` the same way.
#[ur::tool(description = "Negate an integer.", value = "the integer to negate")]
fn negate(value: i64) -> i64 {
    -value
}

/// A tool that fails, to exercise error stringification.
#[ur::tool]
async fn boom(message: String) -> Result<i64, std::io::Error> {
    Err(std::io::Error::other(message))
}

/// A tool whose advertised name overrides the function identifier.
#[ur::tool(name = "custom-name")]
async fn renamed() -> i64 {
    0
}

/// A tool with an optional parameter.
#[ur::tool]
async fn greet(name: String, title: Option<String>) -> String {
    match title {
        Some(title) => format!("Hello, {title} {name}"),
        None => format!("Hello, {name}"),
    }
}

fn done(reason: FinishReason) -> RawEvent {
    RawEvent::Done {
        finish_reason: reason,
        usage: None,
    }
}

#[tokio::test]
async fn sync_and_async_tools_serialize_successful_output() {
    assert_eq!(
        add.call(ToolArguments::from(r#"{"a":41,"b":1}"#)).await,
        Ok("42".to_owned())
    );
    assert_eq!(
        negate.call(ToolArguments::from(r#"{"value":7}"#)).await,
        Ok("-7".to_owned())
    );

    let weather_json = weather
        .call(ToolArguments::from(r#"{"city":"Paris"}"#))
        .await
        .unwrap();
    let value: serde_json::Value = serde_json::from_str(&weather_json).unwrap();
    assert_eq!(value["temp_c"], 18.5);
    assert_eq!(value["summary"], "clear skies over Paris");
}

#[tokio::test]
async fn malformed_arguments_are_stringified_errors() {
    let error = add
        .call(ToolArguments::from("not json"))
        .await
        .expect_err("malformed arguments are a tool error");
    // The serde error message is surfaced verbatim, not an empty placeholder.
    assert!(
        error.contains("column"),
        "expected a serde_json diagnostic, got {error:?}"
    );
}

#[tokio::test]
async fn tool_errors_are_stringified() {
    let result = boom
        .call(ToolArguments::from(r#"{"message":"kaboom"}"#))
        .await;
    assert_eq!(result, Err("kaboom".to_owned()));
}

#[tokio::test]
async fn optional_parameter_is_omittable() {
    assert_eq!(
        greet.call(ToolArguments::from(r#"{"name":"Ada"}"#)).await,
        Ok(r#""Hello, Ada""#.to_owned())
    );
    assert_eq!(
        greet
            .call(ToolArguments::from(r#"{"name":"Ada","title":"Dr."}"#))
            .await,
        Ok(r#""Hello, Dr. Ada""#.to_owned())
    );
}

#[test]
fn generated_schema_matches_real_tool_schema() {
    let schema = add.schema();
    assert_eq!(schema.name, "add");
    assert_eq!(schema.description.as_deref(), Some("Add two integers."));
    assert!(!schema.strict);

    let properties = &schema.parameters["properties"];
    assert!(properties.get("a").is_some());
    assert!(properties.get("b").is_some());

    let required = schema.parameters["required"].as_array().unwrap();
    assert!(required.iter().any(|v| v == "a"));
    assert!(required.iter().any(|v| v == "b"));
}

#[test]
fn optional_parameter_is_not_required_in_schema() {
    let schema = greet.schema();
    let required = schema.parameters["required"].as_array().unwrap();
    assert!(required.iter().any(|v| v == "name"));
    assert!(!required.iter().any(|v| v == "title"));
    assert!(schema.parameters["properties"].get("title").is_some());
}

#[test]
fn parameter_description_is_folded_into_schema() {
    let schema = negate.schema();
    assert_eq!(
        schema.parameters["properties"]["value"]["description"],
        "the integer to negate"
    );
}

#[test]
fn name_override_changes_advertised_name_only() {
    // The struct keeps the function identifier; the tool name is overridden.
    assert_eq!(renamed.name(), "custom-name");
    assert_eq!(renamed.schema().name, "custom-name");
}

#[test]
fn generated_call_uses_ur_signature_types() {
    // Compiles only if `schema`/`call` use the `ur::` re-exported names.
    fn assert_signature<T: Tool>(tool: &T) -> ur::ToolSchema {
        let _future: ur::BoxFuture<'static, Result<String, String>> =
            tool.call(ur::ToolArguments::from("{}"));
        tool.schema()
    }
    let _ = assert_signature(&add);
}

#[tokio::test]
async fn generated_tools_register_and_run_through_the_agent_loop() {
    let provider = FakeProvider::new([
        vec![
            RawEvent::ToolCallDelta {
                index: 0,
                id: Some("call-1".to_owned()),
                name: Some("add".to_owned()),
                arguments: r#"{"a":41,"b":1}"#.to_owned(),
            },
            done(FinishReason::ToolCalls),
        ],
        vec![
            RawEvent::TextDelta("The answer is 42.".to_owned()),
            done(FinishReason::Stop),
        ],
    ]);

    let model = Model::new(provider, "fake-model");
    let agent = Agent::new("You are concise.", model)
        .tool(add)
        .tool(weather);
    let mut session = agent.session();

    let mut tool_outputs = Vec::new();
    let mut text = String::new();
    let mut events = session.send("What is 41 + 1?");
    while let Some(event) = events.next().await {
        match event.unwrap() {
            Event::TextDelta { delta } => text.push_str(&delta),
            Event::ToolResult { output, .. } => tool_outputs.push(output),
            _ => {}
        }
    }
    drop(events);

    assert_eq!(tool_outputs, vec![ToolOutput::Ok("42".to_owned())]);
    assert_eq!(text, "The answer is 42.");
    assert_eq!(
        session.history().last().unwrap().content(),
        Some("The answer is 42.")
    );
}

/// Visibility and doc comments are forwarded to the generated type: under
/// `deny(missing_docs)` a `pub` tool only compiles if its doc comment survived.
mod attribute_preservation {
    #![deny(missing_docs)]

    /// This documentation must be forwarded onto the generated `pub` tool type.
    #[ur::tool(description = "A documented, public tool.")]
    pub async fn documented(x: i64) -> i64 {
        x
    }

    /// A `#[cfg]`-gated tool compiles only if the cfg is replayed onto both the
    /// generated struct and its impl.
    #[cfg(test)]
    #[ur::tool]
    pub async fn gated(x: i64) -> i64 {
        x
    }
}

#[test]
fn public_tools_are_reachable_across_modules() {
    use ur::Tool;
    assert_eq!(attribute_preservation::documented.name(), "documented");
    assert_eq!(attribute_preservation::gated.name(), "gated");
}

/// The DeepSeek provider crate is re-exported under `ur::deepseek` when the
/// `deepseek` feature is enabled. Its absence without the feature is locked by a
/// compile-fail fixture in `compile_contracts.rs`.
#[cfg(feature = "deepseek")]
#[test]
fn deepseek_module_is_exposed_with_feature() {
    fn assert_provider<P: ur::Provider>() {}
    assert_provider::<ur::deepseek::DeepSeekClient>();
}
