//! Encoding helpers shared by providers that speak the OpenAI Chat Completions
//! request dialect.

use serde_json::{Map, Value, json};
use ur_core::Error;
use ur_core::model::ResponseFormat;
use ur_core::provider::{Message, MessageRole, Settings};
use ur_core::schema::strict_schema;
use ur_core::tool::ToolSchema;

/// Encodes an optional string field as a JSON string or `null`.
pub fn content_value(content: Option<&str>) -> Value {
    content.map_or(Value::Null, |text| Value::String(text.to_owned()))
}

/// Encodes a slice of messages into the Chat Completions `messages` array.
pub fn encode_messages(messages: &[Message]) -> Value {
    Value::Array(messages.iter().map(encode_message).collect())
}

/// Encodes a single message into its Chat Completions object.
pub fn encode_message(message: &Message) -> Value {
    let mut object = Map::new();

    match message.role() {
        MessageRole::System => {
            object.insert("role".to_owned(), Value::String("system".to_owned()));
            object.insert("content".to_owned(), content_value(message.content()));
        }
        MessageRole::User => {
            object.insert("role".to_owned(), Value::String("user".to_owned()));
            object.insert("content".to_owned(), content_value(message.content()));
        }
        MessageRole::Assistant => {
            object.insert("role".to_owned(), Value::String("assistant".to_owned()));
            object.insert("content".to_owned(), content_value(message.content()));
            if !message.tool_calls().is_empty() {
                let calls = message
                    .tool_calls()
                    .iter()
                    .map(|call| {
                        json!({
                            "id": call.id,
                            "type": "function",
                            "function": {
                                "name": call.name,
                                "arguments": call.arguments.as_str(),
                            },
                        })
                    })
                    .collect();
                object.insert("tool_calls".to_owned(), Value::Array(calls));
            }
        }
        MessageRole::Tool => {
            object.insert("role".to_owned(), Value::String("tool".to_owned()));
            object.insert(
                "tool_call_id".to_owned(),
                content_value(message.tool_call_id()),
            );
            object.insert("content".to_owned(), content_value(message.content()));
        }
    }

    Value::Object(object)
}

/// Encodes the tool list into the `tools` array, with each tool's `strict` flag
/// honored individually. Returns `None` when there are no tools.
pub fn encode_tools(tools: &[ToolSchema]) -> Option<Value> {
    if tools.is_empty() {
        return None;
    }

    Some(Value::Array(tools.iter().map(encode_tool).collect()))
}

fn encode_tool(tool: &ToolSchema) -> Value {
    let parameters = if tool.strict {
        strict_schema(&tool.parameters)
    } else {
        tool.parameters.clone()
    };

    let mut function = Map::new();
    function.insert("name".to_owned(), Value::String(tool.name.clone()));
    if let Some(description) = &tool.description {
        function.insert("description".to_owned(), Value::String(description.clone()));
    }
    function.insert("parameters".to_owned(), parameters);
    function.insert("strict".to_owned(), Value::Bool(tool.strict));

    json!({
        "type": "function",
        "function": Value::Object(function),
    })
}

/// Encodes the `response_format` field for the native OpenAI shape (`text`,
/// `json_object`, or a rewritten `json_schema`).
pub fn encode_response_format(body: &mut Map<String, Value>, format: &ResponseFormat) {
    match format {
        ResponseFormat::Text => {}
        ResponseFormat::JsonObject => {
            body.insert(
                "response_format".to_owned(),
                json!({ "type": "json_object" }),
            );
        }
        ResponseFormat::JsonSchema(format) => {
            let schema = if format.strict {
                strict_schema(&format.schema)
            } else {
                format.schema.clone()
            };

            let mut json_schema = Map::new();
            json_schema.insert("name".to_owned(), Value::String(format.name.clone()));
            if let Some(description) = &format.description {
                json_schema.insert("description".to_owned(), Value::String(description.clone()));
            }
            json_schema.insert("schema".to_owned(), schema);
            json_schema.insert("strict".to_owned(), Value::Bool(format.strict));

            body.insert(
                "response_format".to_owned(),
                json!({ "type": "json_schema", "json_schema": Value::Object(json_schema) }),
            );
        }
        _ => {}
    }
}

/// Encodes `stop` sequences, rejecting more than `max` of them.
pub fn encode_stop(
    body: &mut Map<String, Value>,
    settings: &Settings,
    max: usize,
) -> Result<(), Error> {
    if settings.stop.is_empty() {
        return Ok(());
    }

    if settings.stop.len() > max {
        return Err(Error::Config {
            message: format!(
                "at most {max} stop sequences are allowed, got {}",
                settings.stop.len()
            ),
        });
    }

    body.insert("stop".to_owned(), json!(settings.stop));
    Ok(())
}

/// Encodes `temperature` and `top_p`, validating their ranges.
pub fn encode_sampling(body: &mut Map<String, Value>, settings: &Settings) -> Result<(), Error> {
    if let Some(temperature) = settings.temperature {
        if !(0.0..=2.0).contains(&temperature) {
            return Err(Error::Config {
                message: format!("temperature {temperature} is outside the range 0.0..=2.0"),
            });
        }
        body.insert("temperature".to_owned(), json!(temperature));
    }

    if let Some(top_p) = settings.top_p {
        if !(0.0..=1.0).contains(&top_p) {
            return Err(Error::Config {
                message: format!("top_p {top_p} is outside the range 0.0..=1.0"),
            });
        }
        body.insert("top_p".to_owned(), json!(top_p));
    }

    Ok(())
}
