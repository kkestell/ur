//! Encoding of a core [`Request`] into the OpenAI Chat Completions body.

use serde_json::{Map, Value, json};
use ur_core::Error;
use ur_core::model::ReasoningEffort;
use ur_core::provider::{Request, Settings};
use ur_openai_compat::request::{
    encode_messages, encode_response_format, encode_sampling, encode_stop, encode_tools,
};

const MAX_STOP_SEQUENCES: usize = 4;

pub(crate) fn encode(request: &Request, user: Option<&str>) -> Result<Value, Error> {
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

    if let Some(user) = user {
        body.insert("user".to_owned(), Value::String(user.to_owned()));
    }

    Ok(Value::Object(body))
}

fn encode_settings(body: &mut Map<String, Value>, settings: &Settings) -> Result<(), Error> {
    if let Some(effort) = settings.reasoning_effort {
        body.insert(
            "reasoning_effort".to_owned(),
            Value::String(reasoning_effort(effort).to_owned()),
        );
    }

    if let Some(max_tokens) = settings.max_tokens {
        if max_tokens == 0 {
            return Err(Error::Config {
                message: "max_tokens must be at least 1".to_owned(),
            });
        }
        body.insert("max_completion_tokens".to_owned(), Value::from(max_tokens));
    }

    encode_stop(body, settings, MAX_STOP_SEQUENCES)?;
    encode_response_format(body, &settings.response_format);
    encode_sampling(body, settings)?;

    Ok(())
}

fn reasoning_effort(effort: ReasoningEffort) -> &'static str {
    match effort {
        ReasoningEffort::Low => "low",
        ReasoningEffort::Medium => "medium",
        ReasoningEffort::High | ReasoningEffort::ExtraHigh | ReasoningEffort::Max => "high",
        _ => "medium",
    }
}

#[cfg(all(test, feature = "serde"))]
mod tests {
    use super::*;
    use ur_core::model::{ResponseFormat, Thinking};
    use ur_core::provider::{Message, ToolCall};
    use ur_core::tool::ToolSchema;

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
            "gpt-5.5",
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
        let body = encode(&user_turn(Settings::default()), None).unwrap();
        assert_eq!(
            body,
            json!({
                "model": "gpt-5.5",
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
                "gpt-5.5",
                vec![Message::user("hi")],
                vec![tool],
                Settings::default(),
            ),
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
                "gpt-5.5",
                vec![Message::user("hi")],
                vec![strict, loose],
                Settings::default(),
            ),
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
    fn settings_are_encoded_with_openai_names() {
        let mut settings = Settings::default();
        settings.thinking = Thinking::Enabled;
        settings.max_tokens = Some(128);
        settings.temperature = Some(0.7);
        settings.top_p = Some(0.9);
        settings.stop = vec!["STOP".to_owned()];
        settings.response_format = ResponseFormat::JsonObject;
        settings.reasoning_effort = Some(ReasoningEffort::ExtraHigh);

        let body = encode(&user_turn(settings), Some("tenant-1")).unwrap();
        assert!(body.get("thinking").is_none());
        assert_eq!(body["max_completion_tokens"], json!(128));
        assert_eq!(body["temperature"], json!(0.7_f32));
        assert_eq!(body["top_p"], json!(0.9_f32));
        assert_eq!(body["stop"], json!(["STOP"]));
        assert_eq!(body["response_format"], json!({ "type": "json_object" }));
        assert_eq!(body["reasoning_effort"], json!("high"));
        assert_eq!(body["user"], json!("tenant-1"));
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

        let body = encode(&user_turn(settings), None).unwrap();
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

        let body = encode(&user_turn(settings), None).unwrap();
        let json_schema = &body["response_format"]["json_schema"];
        assert_eq!(json_schema["strict"], json!(false));
        assert!(json_schema.get("description").is_none());
        assert_eq!(json_schema["schema"], raw);
    }

    #[test]
    fn reasoning_effort_maps_supported_levels_directly() {
        let cases = [
            (ReasoningEffort::Low, "low"),
            (ReasoningEffort::Medium, "medium"),
            (ReasoningEffort::High, "high"),
            (ReasoningEffort::ExtraHigh, "high"),
            (ReasoningEffort::Max, "high"),
        ];
        for (effort, expected) in cases {
            let mut settings = Settings::default();
            settings.reasoning_effort = Some(effort);
            let body = encode(&user_turn(settings), None).unwrap();
            assert_eq!(body["reasoning_effort"], json!(expected));
        }
    }

    #[test]
    fn invalid_settings_are_config_errors() {
        let mut zero = Settings::default();
        zero.max_tokens = Some(0);
        assert_config(encode(&user_turn(zero), None));

        let mut bad_temp = Settings::default();
        bad_temp.temperature = Some(2.1);
        assert_config(encode(&user_turn(bad_temp), None));

        let mut bad_top_p = Settings::default();
        bad_top_p.top_p = Some(1.1);
        assert_config(encode(&user_turn(bad_top_p), None));

        let mut too_many_stops = Settings::default();
        too_many_stops.stop = (0..5).map(|n| n.to_string()).collect();
        assert_config(encode(&user_turn(too_many_stops), None));
    }

    #[test]
    fn assistant_tool_history_round_trips_without_reasoning_content() {
        let call = ToolCall::new("call-1", "add", r#"{"a":1}"#);
        let assistant = Message::assistant(
            Some("ok".to_owned()),
            Some("ignored by OpenAI".to_owned()),
            vec![call],
        );
        let body = encode(
            &request(
                "gpt-5.5",
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
