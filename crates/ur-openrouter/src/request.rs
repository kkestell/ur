//! Encoding of a core [`Request`] into the OpenRouter Chat Completions body.

use serde_json::{Map, Value, json};
use ur_core::Error;
use ur_core::model::{ReasoningEffort, ResponseFormat, Thinking};
use ur_core::provider::{Message, MessageRole, Request, Settings};
use ur_core::schema::strict_schema;
use ur_core::tool::ToolSchema;

use crate::client::ProviderRouting;

const MAX_STOP_SEQUENCES: usize = 4;

pub(crate) fn encode(
    request: &Request,
    user: Option<&str>,
    provider_routing: Option<&ProviderRouting>,
) -> Result<Value, Error> {
    let mut body = Map::new();

    body.insert("model".to_owned(), Value::String(request.model.clone()));
    body.insert("messages".to_owned(), encode_messages(&request.messages));
    body.insert("stream".to_owned(), Value::Bool(true));
    body.insert(
        "stream_options".to_owned(),
        json!({ "include_usage": true }),
    );

    encode_settings(&mut body, &request.settings)?;

    if let Some(tools) = encode_tools(&request.tools) {
        body.insert("tools".to_owned(), tools);
        body.insert("tool_choice".to_owned(), Value::String("auto".to_owned()));
    }

    if let Some(routing) = provider_routing {
        encode_provider_routing(&mut body, routing);
    }

    if let Some(user) = user {
        body.insert("user".to_owned(), Value::String(user.to_owned()));
    }

    Ok(Value::Object(body))
}

fn encode_settings(body: &mut Map<String, Value>, settings: &Settings) -> Result<(), Error> {
    encode_reasoning(body, settings);

    if let Some(max_tokens) = settings.max_tokens {
        if max_tokens == 0 {
            return Err(Error::Config {
                message: "max_tokens must be at least 1".to_owned(),
            });
        }
        body.insert("max_completion_tokens".to_owned(), Value::from(max_tokens));
    }

    encode_stop(body, settings)?;
    encode_response_format(body, &settings.response_format);
    encode_sampling(body, settings)?;

    Ok(())
}

// OpenRouter normalizes reasoning into a single `reasoning` object rather than
// OpenAI's flat `reasoning_effort` string, letting `thinking` and
// `reasoning_effort` be expressed together.
fn encode_reasoning(body: &mut Map<String, Value>, settings: &Settings) {
    let mut reasoning = Map::new();

    match settings.thinking {
        Thinking::Default => {}
        Thinking::Enabled => {
            reasoning.insert("enabled".to_owned(), Value::Bool(true));
        }
        Thinking::Disabled => {
            reasoning.insert("enabled".to_owned(), Value::Bool(false));
        }
        _ => {}
    }

    if let Some(effort) = settings.reasoning_effort {
        reasoning.insert(
            "effort".to_owned(),
            Value::String(reasoning_effort(effort).to_owned()),
        );
    }

    if !reasoning.is_empty() {
        body.insert("reasoning".to_owned(), Value::Object(reasoning));
    }
}

fn encode_provider_routing(body: &mut Map<String, Value>, routing: &ProviderRouting) {
    let mut provider = Map::new();

    if !routing.order.is_empty() {
        provider.insert("order".to_owned(), json!(routing.order));
    }
    if let Some(allow_fallbacks) = routing.allow_fallbacks {
        provider.insert("allow_fallbacks".to_owned(), Value::Bool(allow_fallbacks));
    }
    if let Some(sort) = &routing.sort {
        provider.insert("sort".to_owned(), Value::String(sort.clone()));
    }
    if !routing.only.is_empty() {
        provider.insert("only".to_owned(), json!(routing.only));
    }
    if !routing.ignore.is_empty() {
        provider.insert("ignore".to_owned(), json!(routing.ignore));
    }

    if !provider.is_empty() {
        body.insert("provider".to_owned(), Value::Object(provider));
    }
}

fn encode_response_format(body: &mut Map<String, Value>, format: &ResponseFormat) {
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

fn encode_stop(body: &mut Map<String, Value>, settings: &Settings) -> Result<(), Error> {
    if settings.stop.is_empty() {
        return Ok(());
    }

    if settings.stop.len() > MAX_STOP_SEQUENCES {
        return Err(Error::Config {
            message: format!(
                "at most {MAX_STOP_SEQUENCES} stop sequences are allowed, got {}",
                settings.stop.len()
            ),
        });
    }

    body.insert("stop".to_owned(), json!(settings.stop));
    Ok(())
}

fn encode_sampling(body: &mut Map<String, Value>, settings: &Settings) -> Result<(), Error> {
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

// OpenRouter's reasoning effort scale tops out at `xhigh`, so both `ExtraHigh`
// and `Max` collapse onto it.
fn reasoning_effort(effort: ReasoningEffort) -> &'static str {
    match effort {
        ReasoningEffort::Low => "low",
        ReasoningEffort::Medium => "medium",
        ReasoningEffort::High => "high",
        ReasoningEffort::ExtraHigh | ReasoningEffort::Max => "xhigh",
        _ => "medium",
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
                message
                    .tool_call_id()
                    .map(|id| Value::String(id.to_owned()))
                    .unwrap_or(Value::Null),
            );
            object.insert("content".to_owned(), content_value(message.content()));
        }
    }

    Value::Object(object)
}

fn content_value(content: Option<&str>) -> Value {
    content.map_or(Value::Null, |text| Value::String(text.to_owned()))
}

fn encode_tools(tools: &[ToolSchema]) -> Option<Value> {
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

#[cfg(all(test, feature = "serde"))]
mod tests {
    use super::*;
    use ur_core::provider::ToolCall;

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
            "openai/gpt-5.5",
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
        let body = encode(&user_turn(Settings::default()), None, None).unwrap();
        assert_eq!(
            body,
            json!({
                "model": "openai/gpt-5.5",
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
                "openai/gpt-5.5",
                vec![Message::user("hi")],
                vec![tool],
                Settings::default(),
            ),
            None,
            None,
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
    fn mixed_strict_tools_are_allowed_and_rewritten_individually() {
        let strict = ToolSchema::new(
            "strict_weather",
            json!({
                "type": "object",
                "properties": {
                    "city": { "type": "string", "minLength": 1 },
                    "units": { "type": "string" },
                },
                "required": ["city"],
            }),
        )
        .strict(true);
        let loose = ToolSchema::new("loose_weather", json!({ "type": "object" }));
        let body = encode(
            &request(
                "openai/gpt-5.5",
                vec![Message::user("hi")],
                vec![strict, loose],
                Settings::default(),
            ),
            None,
            None,
        )
        .unwrap();

        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools[0]["function"]["strict"], json!(true));
        assert_eq!(tools[1]["function"]["strict"], json!(false));

        let schema = &tools[0]["function"]["parameters"];
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
    fn settings_are_encoded_with_openrouter_names() {
        let mut settings = Settings::default();
        settings.thinking = Thinking::Enabled;
        settings.max_tokens = Some(128);
        settings.temperature = Some(0.7);
        settings.top_p = Some(0.9);
        settings.stop = vec!["STOP".to_owned()];
        settings.response_format = ResponseFormat::JsonObject;
        settings.reasoning_effort = Some(ReasoningEffort::ExtraHigh);

        let body = encode(&user_turn(settings), Some("tenant-1"), None).unwrap();
        assert_eq!(body["max_completion_tokens"], json!(128));
        assert_eq!(body["temperature"], json!(0.7_f32));
        assert_eq!(body["top_p"], json!(0.9_f32));
        assert_eq!(body["stop"], json!(["STOP"]));
        assert_eq!(body["response_format"], json!({ "type": "json_object" }));
        assert_eq!(
            body["reasoning"],
            json!({ "enabled": true, "effort": "xhigh" })
        );
        assert_eq!(body["user"], json!("tenant-1"));
    }

    #[test]
    fn thinking_disabled_sets_reasoning_enabled_false() {
        let mut settings = Settings::default();
        settings.thinking = Thinking::Disabled;
        let body = encode(&user_turn(settings), None, None).unwrap();
        assert_eq!(body["reasoning"], json!({ "enabled": false }));
    }

    #[test]
    fn default_thinking_without_effort_omits_reasoning() {
        let body = encode(&user_turn(Settings::default()), None, None).unwrap();
        assert!(body.get("reasoning").is_none());
    }

    #[test]
    fn reasoning_effort_maps_to_openrouter_levels() {
        let cases = [
            (ReasoningEffort::Low, "low"),
            (ReasoningEffort::Medium, "medium"),
            (ReasoningEffort::High, "high"),
            (ReasoningEffort::ExtraHigh, "xhigh"),
            (ReasoningEffort::Max, "xhigh"),
        ];
        for (effort, expected) in cases {
            let mut settings = Settings::default();
            settings.reasoning_effort = Some(effort);
            let body = encode(&user_turn(settings), None, None).unwrap();
            assert_eq!(body["reasoning"], json!({ "effort": expected }));
        }
    }

    #[test]
    fn provider_routing_is_encoded_when_present() {
        let routing = ProviderRouting {
            order: vec!["openai".to_owned(), "azure".to_owned()],
            allow_fallbacks: Some(false),
            sort: Some("throughput".to_owned()),
            ignore: vec!["bedrock".to_owned()],
            ..Default::default()
        };
        let body = encode(&user_turn(Settings::default()), None, Some(&routing)).unwrap();
        assert_eq!(
            body["provider"],
            json!({
                "order": ["openai", "azure"],
                "allow_fallbacks": false,
                "sort": "throughput",
                "ignore": ["bedrock"],
            })
        );
    }

    #[test]
    fn no_provider_routing_omits_the_field() {
        let body = encode(&user_turn(Settings::default()), None, None).unwrap();
        assert!(body.get("provider").is_none());
    }

    #[test]
    fn strict_json_schema_is_encoded_and_rewritten() {
        use ur_core::model::JsonSchemaFormat;

        let mut settings = Settings::default();
        settings.response_format = ResponseFormat::JsonSchema(
            JsonSchemaFormat::new(
                "capital",
                json!({
                    "type": "object",
                    "properties": {
                        "city": { "type": "string", "minLength": 1 },
                        "note": { "type": "string" },
                    },
                    "required": ["city"],
                }),
            )
            .description("A capital city."),
        );

        let body = encode(&user_turn(settings), None, None).unwrap();
        let format = &body["response_format"];
        assert_eq!(format["type"], json!("json_schema"));

        let json_schema = &format["json_schema"];
        assert_eq!(json_schema["name"], json!("capital"));
        assert_eq!(json_schema["description"], json!("A capital city."));
        assert_eq!(json_schema["strict"], json!(true));

        let schema = &json_schema["schema"];
        assert_eq!(schema["additionalProperties"], json!(false));
        assert_eq!(schema["required"], json!(["city", "note"]));
        assert!(schema["properties"]["city"].get("minLength").is_none());
        assert_eq!(schema["properties"]["city"]["type"], json!("string"));
        assert_eq!(
            schema["properties"]["note"]["type"],
            json!(["string", "null"])
        );
    }

    #[test]
    fn non_strict_json_schema_is_passed_through_unrewritten() {
        use ur_core::model::JsonSchemaFormat;

        let raw = json!({
            "type": "object",
            "properties": { "city": { "type": "string" } },
            "required": ["city"],
        });
        let mut settings = Settings::default();
        settings.response_format =
            ResponseFormat::JsonSchema(JsonSchemaFormat::new("capital", raw.clone()).strict(false));

        let body = encode(&user_turn(settings), None, None).unwrap();
        let json_schema = &body["response_format"]["json_schema"];
        assert_eq!(json_schema["strict"], json!(false));
        assert!(json_schema.get("description").is_none());
        assert_eq!(json_schema["schema"], raw);
    }

    #[test]
    fn invalid_settings_are_config_errors() {
        let mut zero = Settings::default();
        zero.max_tokens = Some(0);
        assert_config(encode(&user_turn(zero), None, None));

        let mut bad_temp = Settings::default();
        bad_temp.temperature = Some(2.1);
        assert_config(encode(&user_turn(bad_temp), None, None));

        let mut bad_top_p = Settings::default();
        bad_top_p.top_p = Some(1.1);
        assert_config(encode(&user_turn(bad_top_p), None, None));

        let mut too_many_stops = Settings::default();
        too_many_stops.stop = (0..5).map(|n| n.to_string()).collect();
        assert_config(encode(&user_turn(too_many_stops), None, None));
    }

    #[test]
    fn assistant_tool_history_round_trips_without_reasoning_content() {
        let call = ToolCall::new("call-1", "add", r#"{"a":1}"#);
        let assistant = Message::assistant(
            Some("ok".to_owned()),
            Some("ignored on the way back".to_owned()),
            vec![call],
        );
        let body = encode(
            &request(
                "openai/gpt-5.5",
                vec![
                    Message::system("sys"),
                    Message::user("hi"),
                    assistant,
                    Message::tool("call-1", "2"),
                ],
                Vec::new(),
                Settings::default(),
            ),
            None,
            None,
        )
        .unwrap();

        let messages = body["messages"].as_array().unwrap();
        assert_eq!(
            messages[2],
            json!({
                "role": "assistant",
                "content": "ok",
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
}
