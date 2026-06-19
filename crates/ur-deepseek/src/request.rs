//! Encoding of a core [`Request`] into the DeepSeek `chat/completions` body.

use serde_json::{Map, Value, json};
use ur_core::Error;
use ur_core::model::{ReasoningEffort, ResponseFormat, Thinking};
use ur_core::provider::{Message, MessageRole, Request, Settings};
use ur_core::schema::strict_schema;
use ur_core::tool::ToolSchema;
use ur_openai_compat::request::{content_value, encode_stop};

use crate::catalog;

/// The maximum number of stop sequences DeepSeek accepts.
const MAX_STOP_SEQUENCES: usize = 16;

/// Encodes a provider request into the DeepSeek request body, validating the
/// generation settings and tool set before any request is built.
pub(crate) fn encode(request: &Request, user_id: Option<&str>, beta: bool) -> Result<Value, Error> {
    let mut body = Map::new();

    body.insert("model".to_owned(), Value::String(request.model.clone()));
    body.insert("messages".to_owned(), encode_messages(&request.messages));
    body.insert("stream".to_owned(), Value::Bool(true));
    body.insert(
        "stream_options".to_owned(),
        json!({ "include_usage": true }),
    );

    encode_settings(&mut body, request)?;

    if let Some(tools) = encode_tools(&request.tools, beta)? {
        body.insert("tools".to_owned(), tools);
        body.insert("tool_choice".to_owned(), Value::String("auto".to_owned()));
    }

    if let Some(user_id) = user_id {
        body.insert("user_id".to_owned(), Value::String(user_id.to_owned()));
    }

    Ok(Value::Object(body))
}

fn encode_settings(body: &mut Map<String, Value>, request: &Request) -> Result<(), Error> {
    let settings = &request.settings;

    match settings.thinking {
        Thinking::Default => {}
        Thinking::Enabled => {
            body.insert("thinking".to_owned(), json!({ "type": "enabled" }));
        }
        Thinking::Disabled => {
            body.insert("thinking".to_owned(), json!({ "type": "disabled" }));
        }
        _ => {}
    }

    if let Some(effort) = settings.reasoning_effort {
        body.insert(
            "reasoning_effort".to_owned(),
            Value::String(reasoning_effort(effort).to_owned()),
        );
    }

    encode_max_tokens(body, request)?;
    encode_stop(body, settings, MAX_STOP_SEQUENCES)?;

    match &settings.response_format {
        ResponseFormat::Text => {}
        ResponseFormat::JsonObject => {
            body.insert(
                "response_format".to_owned(),
                json!({ "type": "json_object" }),
            );
        }
        ResponseFormat::JsonSchema(_) => {
            return Err(Error::Config {
                message: "DeepSeek does not support a json_schema response format; \
                    use ResponseFormat::JsonObject"
                    .to_owned(),
            });
        }
        _ => {}
    }

    encode_sampling(body, settings)?;

    Ok(())
}

fn encode_max_tokens(body: &mut Map<String, Value>, request: &Request) -> Result<(), Error> {
    let Some(max_tokens) = request.settings.max_tokens else {
        return Ok(());
    };

    if max_tokens == 0 {
        return Err(Error::Config {
            message: "max_tokens must be at least 1".to_owned(),
        });
    }

    if let Some(spec) = catalog::model_spec(&request.model)
        && max_tokens > spec.max_output
    {
        return Err(Error::Config {
            message: format!(
                "max_tokens {max_tokens} exceeds model max_output {}",
                spec.max_output
            ),
        });
    }

    body.insert("max_tokens".to_owned(), Value::from(max_tokens));
    Ok(())
}

fn encode_sampling(body: &mut Map<String, Value>, settings: &Settings) -> Result<(), Error> {
    if let Some(temperature) = settings.temperature
        && !(0.0..=2.0).contains(&temperature)
    {
        return Err(Error::Config {
            message: format!("temperature {temperature} is outside the range 0.0..=2.0"),
        });
    }

    if let Some(top_p) = settings.top_p
        && !(0.0..=1.0).contains(&top_p)
    {
        return Err(Error::Config {
            message: format!("top_p {top_p} is outside the range 0.0..=1.0"),
        });
    }

    if settings.thinking == Thinking::Disabled {
        if let Some(temperature) = settings.temperature {
            body.insert("temperature".to_owned(), json!(temperature));
        }
        if let Some(top_p) = settings.top_p {
            body.insert("top_p".to_owned(), json!(top_p));
        }
    }

    Ok(())
}

fn reasoning_effort(effort: ReasoningEffort) -> &'static str {
    match effort {
        ReasoningEffort::Low | ReasoningEffort::Medium | ReasoningEffort::High => "high",
        ReasoningEffort::ExtraHigh | ReasoningEffort::Max => "max",
        _ => "high",
    }
}

fn encode_messages(messages: &[Message]) -> Value {
    Value::Array(messages.iter().map(encode_message).collect())
}

fn encode_message(message: &Message) -> Value {
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
            object.insert(
                "reasoning_content".to_owned(),
                content_value(message.reasoning_content()),
            );
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

fn encode_tools(tools: &[ToolSchema], beta: bool) -> Result<Option<Value>, Error> {
    if tools.is_empty() {
        return Ok(None);
    }

    let strict_count = tools.iter().filter(|tool| tool.strict).count();
    let strict = if strict_count == 0 {
        false
    } else if strict_count == tools.len() {
        true
    } else {
        return Err(Error::Config {
            message: "tools must be either all strict or all non-strict".to_owned(),
        });
    };

    if strict && !beta {
        return Err(Error::Config {
            message: "strict-mode tools require the beta base URL".to_owned(),
        });
    }

    let encoded = tools.iter().map(|tool| encode_tool(tool, strict)).collect();

    Ok(Some(Value::Array(encoded)))
}

fn encode_tool(tool: &ToolSchema, strict: bool) -> Value {
    let parameters = if strict {
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
    function.insert("strict".to_owned(), Value::Bool(strict));

    json!({
        "type": "function",
        "function": Value::Object(function),
    })
}

#[cfg(all(test, feature = "serde"))]
mod tests {
    use super::*;
    use ur_core::provider::ToolCall;

    /// Builds a request fixture. `Request` is `#[non_exhaustive]`, so its parts
    /// are assembled through their serde representations.
    fn request(
        model: &str,
        messages: Vec<Message>,
        tools: Vec<ToolSchema>,
        settings: Settings,
    ) -> Request {
        serde_json::from_value(json!({
            "model": model,
            "messages": serde_json::to_value(messages).unwrap(),
            "tools": serde_json::to_value(tools).unwrap(),
            "settings": serde_json::to_value(settings).unwrap(),
        }))
        .expect("request fixture deserializes")
    }

    fn user_turn(settings: Settings) -> Request {
        request(
            "deepseek-v4-pro",
            vec![Message::system("sys"), Message::user("hi")],
            Vec::new(),
            settings,
        )
    }

    fn assert_config(result: Result<Value, Error>) {
        assert!(
            matches!(result, Err(Error::Config { .. })),
            "expected config error, got {result:?}"
        );
    }

    #[test]
    fn no_tool_request_has_the_streaming_envelope() {
        let body = encode(&user_turn(Settings::default()), None, false).unwrap();
        assert_eq!(
            body,
            json!({
                "model": "deepseek-v4-pro",
                "messages": [
                    { "role": "system", "content": "sys" },
                    { "role": "user", "content": "hi" },
                ],
                "stream": true,
                "stream_options": { "include_usage": true },
            })
        );
    }

    #[test]
    fn tool_request_declares_functions_with_tool_choice() {
        let parameters = json!({
            "type": "object",
            "properties": { "a": { "type": "integer" } },
            "required": ["a"],
        });
        let tool = ToolSchema::new("add", parameters.clone()).description("Add two integers.");
        let body = encode(
            &request(
                "deepseek-v4-pro",
                vec![Message::user("hi")],
                vec![tool],
                Settings::default(),
            ),
            None,
            false,
        )
        .unwrap();

        assert_eq!(
            body["tools"],
            json!([{
                "type": "function",
                "function": {
                    "name": "add",
                    "description": "Add two integers.",
                    "parameters": parameters,
                    "strict": false,
                },
            }])
        );
        assert_eq!(body["tool_choice"], json!("auto"));
    }

    #[test]
    fn tool_without_description_omits_the_field() {
        let tool = ToolSchema::new("noop", json!({ "type": "object" }));
        let body = encode(
            &request(
                "deepseek-v4-pro",
                vec![Message::user("hi")],
                vec![tool],
                Settings::default(),
            ),
            None,
            false,
        )
        .unwrap();

        assert!(body["tools"][0]["function"].get("description").is_none());
    }

    #[test]
    fn strict_tools_are_rewritten_into_the_strict_subset() {
        let parameters = json!({
            "type": "object",
            "properties": {
                "city": { "type": "string", "minLength": 1 },
                "units": { "type": "string" },
            },
            "required": ["city"],
        });
        let tool = ToolSchema::new("weather", parameters).strict(true);
        let body = encode(
            &request(
                "deepseek-v4-pro",
                vec![Message::user("hi")],
                vec![tool],
                Settings::default(),
            ),
            None,
            true,
        )
        .unwrap();

        let function = &body["tools"][0]["function"];
        assert_eq!(function["strict"], json!(true));

        let schema = &function["parameters"];
        assert_eq!(schema["additionalProperties"], json!(false));
        assert_eq!(schema["required"], json!(["city", "units"]));
        assert!(schema["properties"]["city"].get("minLength").is_none());
        assert_eq!(schema["properties"]["city"]["type"], json!("string"));
        assert_eq!(
            schema["properties"]["units"]["type"],
            json!(["string", "null"])
        );
    }

    #[test]
    fn strict_rewrite_recurses_into_nested_objects_and_arrays() {
        let parameters = json!({
            "type": "object",
            "properties": {
                "filters": {
                    "type": "object",
                    "properties": {
                        "tag": { "type": "string" },
                        "limit": { "type": "integer" },
                    },
                    "required": ["tag"],
                },
                "names": {
                    "type": "array",
                    "items": { "type": "string", "minLength": 1 },
                    "minItems": 1,
                    "maxItems": 5,
                },
            },
            "required": ["filters"],
        });
        let tool = ToolSchema::new("search", parameters).strict(true);
        let body = encode(
            &request(
                "deepseek-v4-pro",
                vec![Message::user("hi")],
                vec![tool],
                Settings::default(),
            ),
            None,
            true,
        )
        .unwrap();

        let schema = &body["tools"][0]["function"]["parameters"];
        assert_eq!(schema["additionalProperties"], json!(false));
        assert_eq!(schema["required"], json!(["filters", "names"]));

        let filters = &schema["properties"]["filters"];
        assert_eq!(filters["type"], json!("object"));
        assert_eq!(filters["additionalProperties"], json!(false));
        assert_eq!(filters["required"], json!(["limit", "tag"]));
        assert_eq!(filters["properties"]["tag"]["type"], json!("string"));
        assert_eq!(
            filters["properties"]["limit"]["type"],
            json!(["integer", "null"])
        );

        let names = &schema["properties"]["names"];
        assert_eq!(names["type"], json!(["array", "null"]));
        assert!(names.get("minItems").is_none());
        assert!(names.get("maxItems").is_none());
        assert_eq!(names["items"], json!({ "type": "string" }));
    }

    #[test]
    fn all_strict_tools_each_carry_the_strict_flag() {
        let tools = vec![
            ToolSchema::new("a", json!({ "type": "object" })).strict(true),
            ToolSchema::new("b", json!({ "type": "object" })).strict(true),
        ];
        let body = encode(
            &request(
                "deepseek-v4-pro",
                vec![Message::user("hi")],
                tools,
                Settings::default(),
            ),
            None,
            true,
        )
        .unwrap();

        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 2);
        assert!(
            tools
                .iter()
                .all(|tool| tool["function"]["strict"] == json!(true))
        );
    }

    #[test]
    fn strict_mode_requires_the_beta_url() {
        let tool = ToolSchema::new("a", json!({ "type": "object" })).strict(true);
        let result = encode(
            &request(
                "deepseek-v4-pro",
                vec![Message::user("hi")],
                vec![tool],
                Settings::default(),
            ),
            None,
            false,
        );
        assert_config(result);
    }

    #[test]
    fn mixed_strict_and_non_strict_tools_are_rejected() {
        let tools = vec![
            ToolSchema::new("a", json!({ "type": "object" })).strict(true),
            ToolSchema::new("b", json!({ "type": "object" })),
        ];
        let result = encode(
            &request(
                "deepseek-v4-pro",
                vec![Message::user("hi")],
                tools,
                Settings::default(),
            ),
            None,
            true,
        );
        assert_config(result);
    }

    #[test]
    fn thinking_enabled_emits_thinking_and_omits_sampling() {
        let mut settings = Settings::default();
        settings.thinking = Thinking::Enabled;
        settings.temperature = Some(0.7);
        settings.top_p = Some(0.9);

        let body = encode(&user_turn(settings), None, false).unwrap();
        assert_eq!(body["thinking"], json!({ "type": "enabled" }));
        assert!(body.get("temperature").is_none());
        assert!(body.get("top_p").is_none());
    }

    #[test]
    fn thinking_disabled_emits_sampling() {
        let mut settings = Settings::default();
        settings.thinking = Thinking::Disabled;
        settings.temperature = Some(0.7);
        settings.top_p = Some(0.9);

        let body = encode(&user_turn(settings), None, false).unwrap();
        assert_eq!(body["thinking"], json!({ "type": "disabled" }));
        assert_eq!(body["temperature"], json!(0.7_f32));
        assert_eq!(body["top_p"], json!(0.9_f32));
    }

    #[test]
    fn reasoning_effort_aliases_to_high_or_max() {
        let cases = [
            (ReasoningEffort::Low, "high"),
            (ReasoningEffort::Medium, "high"),
            (ReasoningEffort::High, "high"),
            (ReasoningEffort::ExtraHigh, "max"),
            (ReasoningEffort::Max, "max"),
        ];
        for (effort, expected) in cases {
            let mut settings = Settings::default();
            settings.reasoning_effort = Some(effort);
            let body = encode(&user_turn(settings), None, false).unwrap();
            assert_eq!(body["reasoning_effort"], json!(expected));
        }
    }

    #[test]
    fn json_response_format_is_encoded() {
        let mut settings = Settings::default();
        settings.response_format = ResponseFormat::JsonObject;
        let body = encode(&user_turn(settings), None, false).unwrap();
        assert_eq!(body["response_format"], json!({ "type": "json_object" }));
    }

    #[test]
    fn json_schema_response_format_is_rejected() {
        let mut settings = Settings::default();
        settings.response_format =
            ResponseFormat::json_schema("capital", json!({ "type": "object" }));
        assert_config(encode(&user_turn(settings), None, false));
    }

    #[test]
    fn stop_sequences_are_encoded_and_bounded() {
        let mut settings = Settings::default();
        settings.stop = vec!["\n\n".to_owned(), "STOP".to_owned()];
        let body = encode(&user_turn(settings), None, false).unwrap();
        assert_eq!(body["stop"], json!(["\n\n", "STOP"]));

        let mut too_many = Settings::default();
        too_many.stop = (0..17).map(|n| n.to_string()).collect();
        assert_config(encode(&user_turn(too_many), None, false));
    }

    #[test]
    fn user_id_is_included_when_set() {
        let body = encode(&user_turn(Settings::default()), Some("tenant-1"), false).unwrap();
        assert_eq!(body["user_id"], json!("tenant-1"));

        let absent = encode(&user_turn(Settings::default()), None, false).unwrap();
        assert!(absent.get("user_id").is_none());
    }

    #[test]
    fn reasoning_content_and_tool_calls_round_trip_into_assistant_messages() {
        let call = ToolCall::new("call-1", "add", r#"{"a":1}"#);
        let assistant = Message::assistant(
            Some("ok".to_owned()),
            Some("thinking".to_owned()),
            vec![call],
        );
        let messages = vec![
            Message::system("sys"),
            Message::user("hi"),
            assistant,
            Message::tool("call-1", "2"),
        ];
        let body = encode(
            &request("deepseek-v4-pro", messages, Vec::new(), Settings::default()),
            None,
            false,
        )
        .unwrap();

        let messages = body["messages"].as_array().unwrap();
        assert_eq!(
            messages[2],
            json!({
                "role": "assistant",
                "content": "ok",
                "reasoning_content": "thinking",
                "tool_calls": [{
                    "id": "call-1",
                    "type": "function",
                    "function": { "name": "add", "arguments": r#"{"a":1}"# },
                }],
            })
        );
        assert_eq!(
            messages[3],
            json!({ "role": "tool", "tool_call_id": "call-1", "content": "2" })
        );
    }

    #[test]
    fn assistant_without_reasoning_or_tools_emits_null_content() {
        let assistant = Message::assistant(Some("answer".to_owned()), None, Vec::new());
        let body = encode(
            &request(
                "deepseek-v4-pro",
                vec![Message::user("hi"), assistant],
                Vec::new(),
                Settings::default(),
            ),
            None,
            false,
        )
        .unwrap();

        let assistant = &body["messages"][1];
        assert_eq!(assistant["reasoning_content"], Value::Null);
        assert!(assistant.get("tool_calls").is_none());
    }

    #[test]
    fn max_tokens_is_validated_against_the_catalog() {
        let mut zero = Settings::default();
        zero.max_tokens = Some(0);
        assert_config(encode(&user_turn(zero), None, false));

        let mut over = Settings::default();
        over.max_tokens = Some(384_001);
        assert_config(encode(&user_turn(over), None, false));

        let mut at_cap = Settings::default();
        at_cap.max_tokens = Some(384_000);
        let body = encode(&user_turn(at_cap), None, false).unwrap();
        assert_eq!(body["max_tokens"], json!(384_000));
    }

    #[test]
    fn unknown_model_has_no_max_tokens_cap() {
        let mut settings = Settings::default();
        settings.max_tokens = Some(10_000_000);
        let body = encode(
            &request(
                "some-future-model",
                vec![Message::user("hi")],
                Vec::new(),
                settings,
            ),
            None,
            false,
        )
        .unwrap();
        assert_eq!(body["max_tokens"], json!(10_000_000));
    }

    #[test]
    fn out_of_range_sampling_is_rejected() {
        let mut hot = Settings::default();
        hot.temperature = Some(2.5);
        assert_config(encode(&user_turn(hot), None, false));

        let mut wide = Settings::default();
        wide.top_p = Some(1.5);
        assert_config(encode(&user_turn(wide), None, false));
    }
}
