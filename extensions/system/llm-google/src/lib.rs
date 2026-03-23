wit_bindgen::generate!({
    path: "../../../wit",
    world: "llm-extension-http",
    generate_all,
});

use std::cell::RefCell;

use exports::ur::extension::extension::Guest as ExtGuest;
use exports::ur::extension::llm_provider::Guest as LlmGuest;
use exports::ur::extension::llm_streaming_provider::{
    CompletionStream, Guest as LlmStreamingGuest, GuestCompletionStream,
};
use ur::extension::types::{
    Completion, CompletionChunk, ConfigEntry, ConfigSetting, Message, MessagePart, ModelDescriptor,
    SettingDescriptor, SettingEnum, SettingInteger, SettingSchema, ToolCall, ToolDescriptor, Usage,
};
use wasi::http::outgoing_handler;
use wasi::http::types::{Fields, IncomingBody, Method, OutgoingBody, OutgoingRequest, Scheme};
use wasi::io::streams::StreamError;

thread_local! {
    static API_KEY: RefCell<Option<String>> = const { RefCell::new(None) };
}

fn get_api_key() -> Result<String, String> {
    API_KEY.with(|k| {
        k.borrow()
            .clone()
            .ok_or_else(|| "GOOGLE_API_KEY not configured".into())
    })
}

struct LlmGoogle;

const GEMINI_3_FLASH_PREVIEW: &str = "gemini-3-flash-preview";
const GEMINI_3_1_PRO_PREVIEW: &str = "gemini-3.1-pro-preview";
const GEMINI_3_1_FLASH_LITE_PREVIEW: &str = "gemini-3.1-flash-lite-preview";

// Gemini 3.1 text models advertise up to 64k output tokens in the current docs.
const GOOGLE_MAX_OUTPUT_TOKENS: i64 = 65_536;
const GOOGLE_DEFAULT_MAX_OUTPUT_TOKENS: i64 = 8_192;

const FLASH_THINKING_LEVELS: &[&str] = &["minimal", "low", "medium", "high"];
const PRO_THINKING_LEVELS: &[&str] = &["low", "medium", "high"];
const FLASH_LITE_THINKING_LEVELS: &[&str] = &["minimal", "low", "medium", "high"];

// ── Extension lifecycle ──────────────────────────────────────────────

impl ExtGuest for LlmGoogle {
    fn init(config: Vec<ConfigEntry>) -> Result<(), String> {
        for entry in config {
            if entry.key == "api_key" {
                API_KEY.with(|k| *k.borrow_mut() = Some(entry.value));
            }
        }
        Ok(())
    }

    fn call_tool(_name: String, _args_json: String) -> Result<String, String> {
        Err("no tools implemented".into())
    }

    fn id() -> String {
        "llm-google".into()
    }

    fn name() -> String {
        "Google Gemini".into()
    }

    fn list_tools() -> Vec<ToolDescriptor> {
        vec![]
    }
}

// ── LLM provider ────────────────────────────────────────────────────

impl LlmGuest for LlmGoogle {
    fn provider_id() -> String {
        "google".into()
    }

    fn list_models() -> Vec<ModelDescriptor> {
        const MODELS: &[ModelMeta] = &[
            ModelMeta {
                id: GEMINI_3_FLASH_PREVIEW,
                name: "Gemini 3 Flash Preview",
                description: "Gemini 3 Flash preview with 1M context, 64k output, and Jan 2025 knowledge.",
                is_default: true,
                thinking_levels: FLASH_THINKING_LEVELS,
                default_thinking_level: "high",
                context_window_in: 1_048_576,
                context_window_out: 65_536,
                knowledge_cutoff: "2025-01",
                cost_in: 500,
                cost_out: 3000,
            },
            ModelMeta {
                id: GEMINI_3_1_PRO_PREVIEW,
                name: "Gemini 3.1 Pro Preview",
                description: "Gemini 3.1 Pro preview with 1M context, 64k output, and Jan 2025 knowledge.",
                is_default: false,
                thinking_levels: PRO_THINKING_LEVELS,
                default_thinking_level: "high",
                context_window_in: 1_048_576,
                context_window_out: 65_536,
                knowledge_cutoff: "2025-01",
                cost_in: 2000,
                cost_out: 12000,
            },
            ModelMeta {
                id: GEMINI_3_1_FLASH_LITE_PREVIEW,
                name: "Gemini 3.1 Flash-Lite Preview",
                description: "Gemini 3.1 Flash-Lite preview with 1M context, 64k output, and Jan 2025 knowledge.",
                is_default: false,
                thinking_levels: FLASH_LITE_THINKING_LEVELS,
                default_thinking_level: "minimal",
                context_window_in: 1_048_576,
                context_window_out: 65_536,
                knowledge_cutoff: "2025-01",
                cost_in: 250,
                cost_out: 1500,
            },
        ];

        MODELS.iter().map(model_descriptor).collect()
    }

    fn complete(
        messages: Vec<Message>,
        model: String,
        settings: Vec<ConfigSetting>,
        tools: Vec<ToolDescriptor>,
    ) -> Result<Completion, String> {
        let api_key = get_api_key()?;
        let body = build_request_body(&messages, &settings, &tools);

        let url = format!("/v1beta/models/{model}:generateContent");

        let response_bytes = http_post(&api_key, &url, &body)?;
        let response_str =
            String::from_utf8(response_bytes).map_err(|e| format!("invalid UTF-8: {e}"))?;
        parse_generate_content_response(&response_str)
    }
}

// ── Streaming provider ──────────────────────────────────────────────

struct GoogleStream {
    inner: RefCell<StreamState>,
}

struct StreamState {
    buffer: String,
    pos: usize,
    done: bool,
    body_stream: Option<wasi::io::streams::InputStream>,
    _incoming_body: Option<IncomingBody>,
}

impl LlmStreamingGuest for LlmGoogle {
    type CompletionStream = GoogleStream;

    fn complete_streaming(
        messages: Vec<Message>,
        model: String,
        settings: Vec<ConfigSetting>,
        tools: Vec<ToolDescriptor>,
    ) -> Result<CompletionStream, String> {
        let api_key = get_api_key()?;
        let body = build_request_body(&messages, &settings, &tools);

        let url = format!("/v1beta/models/{model}:streamGenerateContent?alt=sse");

        let (incoming_body, body_stream) = http_post_streaming(&api_key, &url, &body)?;

        Ok(CompletionStream::new(GoogleStream {
            inner: RefCell::new(StreamState {
                buffer: String::new(),
                pos: 0,
                done: false,
                body_stream: Some(body_stream),
                _incoming_body: Some(incoming_body),
            }),
        }))
    }
}

impl GuestCompletionStream for GoogleStream {
    fn next(&self) -> Option<CompletionChunk> {
        let mut state = self.inner.borrow_mut();

        if state.done {
            return None;
        }

        loop {
            // Try to parse a complete SSE event from the buffer.
            if let Some(chunk) = try_parse_sse_event(&mut state) {
                return Some(chunk);
            }

            // Read more data from the stream.
            let stream = state.body_stream.as_ref()?;
            match stream.blocking_read(65536) {
                Ok(bytes) => {
                    if bytes.is_empty() {
                        state.done = true;
                        return None;
                    }
                    let text = String::from_utf8_lossy(&bytes);
                    state.buffer.push_str(&text);
                }
                Err(StreamError::Closed) => {
                    state.done = true;
                    // Try to parse any remaining data.
                    return try_parse_sse_event(&mut state);
                }
                Err(StreamError::LastOperationFailed(e)) => {
                    eprintln!("stream read error: {}", e.to_debug_string());
                    state.done = true;
                    return None;
                }
            }
        }
    }
}

// ── SSE parsing ─────────────────────────────────────────────────────

fn try_parse_sse_event(state: &mut StreamState) -> Option<CompletionChunk> {
    loop {
        let remaining = &state.buffer[state.pos..];
        let (consumed_len, event) = next_complete_sse_event(remaining)?;

        state.pos += consumed_len;
        trim_consumed_buffer(state);

        match parse_sse_event(&event) {
            Ok(Some(chunk)) => return Some(chunk),
            Ok(None) => {}
            Err(error) => {
                eprintln!("skipping malformed SSE event: {error}");
            }
        }
    }
}

fn next_complete_sse_event(buffer: &str) -> Option<(usize, String)> {
    let mut line_start = 0;

    while line_start < buffer.len() {
        let line_rel_end = buffer[line_start..].find('\n')?;
        let line_end = line_start + line_rel_end;
        let line = buffer[line_start..line_end]
            .strip_suffix('\r')
            .unwrap_or(&buffer[line_start..line_end]);
        let next_line_start = line_end + 1;

        if line.is_empty() {
            return Some((next_line_start, buffer[..line_start].to_string()));
        }

        line_start = next_line_start;
    }

    None
}

fn trim_consumed_buffer(state: &mut StreamState) {
    if state.pos == 0 {
        return;
    }

    state.buffer.drain(..state.pos);
    state.pos = 0;
}

fn parse_sse_event(event: &str) -> Result<Option<CompletionChunk>, String> {
    let mut data_lines = Vec::new();

    for raw_line in event.lines() {
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        if line.is_empty() || line.starts_with(':') {
            continue;
        }

        let (field, value) = match line.split_once(':') {
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            None => (line, ""),
        };

        if field == "data" {
            data_lines.push(value);
        }
    }

    if data_lines.is_empty() {
        return Ok(None);
    }

    let payload = data_lines.join("\n");
    if payload == "[DONE]" {
        return Ok(None);
    }

    parse_sse_chunk(&payload).map(Some)
}

fn parse_sse_chunk(json_str: &str) -> Result<CompletionChunk, String> {
    let response: serde_json::Value =
        serde_json::from_str(json_str).map_err(|e| format!("invalid SSE JSON: {e}"))?;

    let candidates = response
        .get("candidates")
        .and_then(|candidates| candidates.as_array())
        .ok_or_else(|| "missing SSE candidates".to_string())?;
    let candidate = candidates
        .first()
        .ok_or_else(|| "missing SSE candidate".to_string())?;
    let content = candidate
        .get("content")
        .ok_or_else(|| "missing SSE content".to_string())?;
    let parts = content
        .get("parts")
        .and_then(|parts| parts.as_array())
        .ok_or_else(|| "missing SSE parts".to_string())?;

    let mut delta_parts = Vec::new();

    for part in parts {
        if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
            delta_parts.push(MessagePart::Text(text.to_string()));
        }
        if let Some(fc) = part.get("functionCall") {
            let name = fc.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let id = fc.get("id").and_then(|i| i.as_str()).unwrap_or("");
            let args = function_call_args_json(fc);
            let metadata = build_provider_metadata(part);
            delta_parts.push(MessagePart::ToolCall(ToolCall {
                id: id.into(),
                name: name.into(),
                arguments_json: args,
                provider_metadata_json: metadata,
            }));
        }
    }

    let finish_reason = candidate
        .get("finishReason")
        .and_then(|r| r.as_str())
        .map(String::from);

    let usage = response.get("usageMetadata").and_then(parse_usage_metadata);

    Ok(CompletionChunk {
        delta_parts,
        finish_reason,
        usage,
    })
}

// ── Request body construction ───────────────────────────────────────

fn build_request_body(
    messages: &[Message],
    settings: &[ConfigSetting],
    tools: &[ToolDescriptor],
) -> String {
    let mut body = serde_json::Map::new();

    // Extract system instruction from messages.
    let system_parts: Vec<&Message> = messages.iter().filter(|m| m.role == "system").collect();
    if !system_parts.is_empty() {
        let system_text: String = system_parts
            .iter()
            .map(|m| extract_text(m))
            .collect::<Vec<_>>()
            .join("\n");
        body.insert(
            "systemInstruction".into(),
            serde_json::json!({
                "parts": [{"text": system_text}]
            }),
        );
    }

    // Build contents (non-system messages).
    let contents: Vec<serde_json::Value> = messages
        .iter()
        .filter(|m| m.role != "system")
        .map(message_to_gemini)
        .collect();
    body.insert("contents".into(), serde_json::Value::Array(contents));

    // Add tool declarations if any tools provided.
    if !tools.is_empty() {
        let declarations: Vec<serde_json::Value> = tools
            .iter()
            .map(|t| {
                let params: serde_json::Value = serde_json::from_str(&t.parameters_json_schema)
                    .unwrap_or(serde_json::json!({"type": "object"}));
                serde_json::json!({
                    "name": t.name,
                    "description": t.description,
                    "parameters": params
                })
            })
            .collect();
        body.insert(
            "tools".into(),
            serde_json::json!([{ "functionDeclarations": declarations }]),
        );
    }

    // Generation config from settings.
    let mut gen_config = serde_json::Map::new();
    let mut thinking_config = serde_json::Map::new();
    for s in settings {
        match s.key.as_str() {
            "max_output_tokens" => {
                if let ur::extension::types::SettingValue::Integer(v) = &s.value {
                    gen_config.insert("maxOutputTokens".into(), serde_json::json!(v));
                }
            }
            "thinking_level" => {
                if let ur::extension::types::SettingValue::Enumeration(level) = &s.value {
                    thinking_config.insert("thinkingLevel".into(), serde_json::json!(level));
                }
            }
            _ => {}
        }
    }
    if !thinking_config.is_empty() {
        gen_config.insert(
            "thinkingConfig".into(),
            serde_json::Value::Object(thinking_config),
        );
    }
    if !gen_config.is_empty() {
        body.insert(
            "generationConfig".into(),
            serde_json::Value::Object(gen_config),
        );
    }

    serde_json::to_string(&body).unwrap_or_default()
}

/// Extracts concatenated text from a message's parts.
fn extract_text(msg: &Message) -> String {
    msg.parts
        .iter()
        .filter_map(|p| match p {
            MessagePart::Text(s) => Some(s.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

/// Captures `thoughtSignature` (if present) into an opaque JSON metadata string.
fn build_provider_metadata(part: &serde_json::Value) -> String {
    match part.get("thoughtSignature").and_then(|s| s.as_str()) {
        Some(sig) => serde_json::json!({"thoughtSignature": sig}).to_string(),
        None => String::new(),
    }
}

fn function_call_args_json(function_call: &serde_json::Value) -> String {
    function_call
        .get("args")
        .map_or_else(|| "{}".into(), std::string::ToString::to_string)
}

fn parse_usage_metadata(usage_metadata: &serde_json::Value) -> Option<Usage> {
    Some(Usage {
        input_tokens: u32::try_from(usage_metadata.get("promptTokenCount")?.as_u64()?).ok()?,
        output_tokens: u32::try_from(usage_metadata.get("candidatesTokenCount")?.as_u64()?).ok()?,
    })
}

/// Extracts `thoughtSignature` from the opaque provider metadata JSON.
fn extract_thought_signature(metadata: &str) -> Option<String> {
    if metadata.is_empty() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(metadata).ok()?;
    v.get("thoughtSignature")
        .and_then(|s| s.as_str())
        .map(String::from)
}

fn message_to_gemini(msg: &Message) -> serde_json::Value {
    let role = match msg.role.as_str() {
        "assistant" => "model",
        _ => "user",
    };

    let parts: Vec<serde_json::Value> = msg
        .parts
        .iter()
        .map(|part| match part {
            MessagePart::Text(s) => serde_json::json!({ "text": s }),
            MessagePart::ToolCall(tc) => {
                let mut part_obj = serde_json::json!({
                    "functionCall": {
                        "name": tc.name,
                        "args": serde_json::from_str::<serde_json::Value>(&tc.arguments_json)
                            .unwrap_or(serde_json::json!({}))
                    }
                });
                // Echo back thoughtSignature if the provider stored one.
                if let Some(sig) = extract_thought_signature(&tc.provider_metadata_json) {
                    part_obj["thoughtSignature"] = serde_json::Value::String(sig);
                }
                part_obj
            }
            MessagePart::ToolResult(tr) => {
                let response: serde_json::Value = serde_json::from_str(&tr.content)
                    .unwrap_or_else(|_| serde_json::json!({"result": tr.content}));
                serde_json::json!({
                    "functionResponse": {
                        "name": tr.tool_name,
                        "id": tr.tool_call_id,
                        "response": response
                    }
                })
            }
        })
        .collect();

    serde_json::json!({ "role": role, "parts": parts })
}

// ── Response parsing ────────────────────────────────────────────────

fn parse_generate_content_response(body: &str) -> Result<Completion, String> {
    let response: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("failed to parse response: {e}"))?;

    // Check for API-level errors.
    if let Some(error) = response.get("error") {
        let msg = error
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown error");
        return Err(format!("Gemini API error: {msg}"));
    }

    let candidates = response
        .get("candidates")
        .and_then(|c| c.as_array())
        .ok_or("no candidates in response")?;

    let candidate = candidates.first().ok_or("empty candidates array")?;
    let content = candidate.get("content").ok_or("no content in candidate")?;
    let parts = content
        .get("parts")
        .and_then(|p| p.as_array())
        .ok_or("no parts in content")?;

    let mut message_parts = Vec::new();

    for part in parts {
        if let Some(t) = part.get("text").and_then(|t| t.as_str()) {
            message_parts.push(MessagePart::Text(t.to_string()));
        }
        if let Some(fc) = part.get("functionCall") {
            let name = fc.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let id = fc.get("id").and_then(|i| i.as_str()).unwrap_or("");
            let args = function_call_args_json(fc);
            let metadata = build_provider_metadata(part);
            message_parts.push(MessagePart::ToolCall(ToolCall {
                id: id.into(),
                name: name.into(),
                arguments_json: args,
                provider_metadata_json: metadata,
            }));
        }
    }

    let usage = response.get("usageMetadata").and_then(parse_usage_metadata);

    Ok(Completion {
        message: Message {
            role: "assistant".into(),
            parts: message_parts,
        },
        usage,
    })
}

// ── WASI HTTP helpers ───────────────────────────────────────────────

fn http_post(api_key: &str, path: &str, body: &str) -> Result<Vec<u8>, String> {
    let (incoming_body, body_stream) = http_post_streaming(api_key, path, body)?;

    // Read entire response body.
    let mut result = Vec::new();
    loop {
        match body_stream.blocking_read(65536) {
            Ok(bytes) => {
                if bytes.is_empty() {
                    break;
                }
                result.extend_from_slice(&bytes);
            }
            Err(StreamError::Closed) => break,
            Err(StreamError::LastOperationFailed(e)) => {
                return Err(format!("read error: {}", e.to_debug_string()));
            }
        }
    }

    drop(body_stream);
    drop(incoming_body);

    Ok(result)
}

fn http_post_streaming(
    api_key: &str,
    path: &str,
    body: &str,
) -> Result<(IncomingBody, wasi::io::streams::InputStream), String> {
    let headers = Fields::from_list(&[
        ("x-goog-api-key".into(), api_key.as_bytes().to_vec()),
        ("content-type".into(), b"application/json".to_vec()),
    ])
    .map_err(|error| format!("failed to create headers: {error:?}"))?;

    let request = OutgoingRequest::new(headers);
    request
        .set_method(&Method::Post)
        .map_err(|()| "failed to set method")?;
    request
        .set_scheme(Some(&Scheme::Https))
        .map_err(|()| "failed to set scheme")?;
    request
        .set_authority(Some("generativelanguage.googleapis.com"))
        .map_err(|()| "failed to set authority")?;
    request
        .set_path_with_query(Some(path))
        .map_err(|()| "failed to set path")?;

    // Write request body.
    let outgoing_body = request
        .body()
        .map_err(|()| "failed to get request body".to_string())?;
    let out_stream = outgoing_body
        .write()
        .map_err(|()| "failed to get output stream".to_string())?;
    out_stream
        .blocking_write_and_flush(body.as_bytes())
        .map_err(|e| format!("write error: {e:?}"))?;
    drop(out_stream);
    OutgoingBody::finish(outgoing_body, None).map_err(|e| format!("body finish error: {e:?}"))?;

    // Send request and wait for response.
    let future_response =
        outgoing_handler::handle(request, None).map_err(|e| format!("request error: {e:?}"))?;

    let pollable = future_response.subscribe();
    pollable.block();

    let response = future_response
        .get()
        .ok_or("response not ready")?
        .map_err(|()| "response already consumed")?
        .map_err(|e| format!("HTTP error: {e:?}"))?;

    let status = response.status();
    if !(200..300).contains(&status) {
        // Try to read error body.
        if let Ok(body) = response.consume()
            && let Ok(stream) = body.stream()
        {
            let mut err_bytes = Vec::new();
            loop {
                match stream.blocking_read(65536) {
                    Ok(bytes) if bytes.is_empty() => break,
                    Ok(bytes) => err_bytes.extend_from_slice(&bytes),
                    Err(_) => break,
                }
            }
            let err_text = String::from_utf8_lossy(&err_bytes);
            return Err(format!("Gemini API error: HTTP {status}: {err_text}"));
        }
        return Err(format!("Gemini API error: HTTP {status}"));
    }

    let incoming_body = response
        .consume()
        .map_err(|()| "failed to consume response body".to_string())?;
    let body_stream = incoming_body
        .stream()
        .map_err(|()| "failed to get response stream".to_string())?;

    Ok((incoming_body, body_stream))
}

// ── Settings helpers ────────────────────────────────────────────────

struct ModelMeta {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    is_default: bool,
    thinking_levels: &'static [&'static str],
    default_thinking_level: &'static str,
    context_window_in: u32,
    context_window_out: u32,
    knowledge_cutoff: &'static str,
    cost_in: u32,
    cost_out: u32,
}

fn model_descriptor(meta: &ModelMeta) -> ModelDescriptor {
    ModelDescriptor {
        id: meta.id.into(),
        name: meta.name.into(),
        description: meta.description.into(),
        is_default: meta.is_default,
        settings: model_settings(meta.thinking_levels, meta.default_thinking_level),
        context_window_in: meta.context_window_in,
        context_window_out: meta.context_window_out,
        knowledge_cutoff: meta.knowledge_cutoff.into(),
        cost_in: meta.cost_in,
        cost_out: meta.cost_out,
    }
}

fn model_settings(
    thinking_levels: &[&str],
    default_thinking_level: &str,
) -> Vec<SettingDescriptor> {
    vec![
        SettingDescriptor {
            key: "max_output_tokens".into(),
            name: "Max Output Tokens".into(),
            description: "Maximum number of tokens to generate".into(),
            schema: SettingSchema::Integer(SettingInteger {
                min: 1,
                max: GOOGLE_MAX_OUTPUT_TOKENS,
                default_val: GOOGLE_DEFAULT_MAX_OUTPUT_TOKENS,
            }),
        },
        SettingDescriptor {
            key: "thinking_level".into(),
            name: "Thinking Level".into(),
            description: "Relative reasoning depth for Gemini 3.1 models".into(),
            schema: SettingSchema::Enumeration(SettingEnum {
                allowed: thinking_levels
                    .iter()
                    .map(|level| (*level).to_owned())
                    .collect(),
                default_val: default_thinking_level.into(),
            }),
        },
    ]
}

export!(LlmGoogle);

#[cfg(test)]
mod tests {
    use super::*;

    fn model_by_id<'a>(models: &'a [ModelDescriptor], id: &str) -> &'a ModelDescriptor {
        models
            .iter()
            .find(|model| model.id == id)
            .expect("model descriptor")
    }

    fn setting_by_key<'a>(settings: &'a [SettingDescriptor], key: &str) -> &'a SettingDescriptor {
        settings
            .iter()
            .find(|setting| setting.key == key)
            .expect("setting descriptor")
    }

    fn text_chunk_json(text: &str) -> String {
        serde_json::json!({
            "candidates": [{
                "content": {
                    "parts": [{
                        "text": text
                    }]
                }
            }]
        })
        .to_string()
    }

    fn stream_state(buffer: &str) -> StreamState {
        StreamState {
            buffer: buffer.into(),
            pos: 0,
            done: false,
            body_stream: None,
            _incoming_body: None,
        }
    }

    #[test]
    fn try_parse_sse_event_keeps_partial_json_buffered() {
        let mut state =
            stream_state("data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"hel");

        let chunk = try_parse_sse_event(&mut state);

        assert!(chunk.is_none());
        assert_eq!(state.pos, 0);
    }

    #[test]
    fn try_parse_sse_event_requires_blank_line_terminator() {
        let json = text_chunk_json("Hello");
        let mut state = stream_state(&format!("data: {json}"));

        let chunk = try_parse_sse_event(&mut state);

        assert!(chunk.is_none());
        assert_eq!(state.pos, 0);
    }

    #[test]
    fn try_parse_sse_event_emits_buffered_events_one_at_a_time() {
        let first = text_chunk_json("Hello");
        let second = text_chunk_json(" world");
        let mut state = stream_state(&format!("data: {first}\n\ndata: {second}\n\n"));

        let first_chunk = try_parse_sse_event(&mut state).expect("first SSE chunk");
        let second_chunk = try_parse_sse_event(&mut state).expect("second SSE chunk");

        assert!(matches!(
            &first_chunk.delta_parts[0],
            MessagePart::Text(text) if text == "Hello"
        ));
        assert!(matches!(
            &second_chunk.delta_parts[0],
            MessagePart::Text(text) if text == " world"
        ));
    }

    #[test]
    fn try_parse_sse_event_accepts_crlf_delimiters() {
        let json = text_chunk_json("Hello");
        let mut state = stream_state(&format!("data: {json}\r\n\r\n"));

        let chunk = try_parse_sse_event(&mut state).expect("CRLF SSE chunk");

        assert!(matches!(
            &chunk.delta_parts[0],
            MessagePart::Text(text) if text == "Hello"
        ));
    }

    #[test]
    fn list_models_advertises_current_gemini_3_text_models() {
        let models = LlmGoogle::list_models();
        let ids: Vec<&str> = models.iter().map(|model| model.id.as_str()).collect();

        assert_eq!(
            ids,
            vec![
                "gemini-3-flash-preview",
                "gemini-3.1-pro-preview",
                "gemini-3.1-flash-lite-preview",
            ]
        );
        assert_eq!(
            models
                .iter()
                .find(|model| model.is_default)
                .map(|model| model.id.as_str()),
            Some("gemini-3-flash-preview")
        );
    }

    #[test]
    fn list_models_exposes_model_specific_thinking_levels() {
        let models = LlmGoogle::list_models();

        let flash = model_by_id(&models, "gemini-3-flash-preview");
        let pro = model_by_id(&models, "gemini-3.1-pro-preview");
        let flash_lite = model_by_id(&models, "gemini-3.1-flash-lite-preview");

        for model in [&flash, &pro, &flash_lite] {
            assert!(
                model
                    .settings
                    .iter()
                    .any(|setting| setting.key == "max_output_tokens")
            );
            assert!(
                !model
                    .settings
                    .iter()
                    .any(|setting| setting.key == "temperature")
            );
        }

        let flash_thinking = setting_by_key(&flash.settings, "thinking_level");
        let pro_thinking = setting_by_key(&pro.settings, "thinking_level");
        let flash_lite_thinking = setting_by_key(&flash_lite.settings, "thinking_level");

        assert!(matches!(
            &flash_thinking.schema,
            SettingSchema::Enumeration(schema)
                if schema.allowed == vec!["minimal", "low", "medium", "high"]
                    && schema.default_val == "high"
        ));
        assert!(matches!(
            &pro_thinking.schema,
            SettingSchema::Enumeration(schema)
                if schema.allowed == vec!["low", "medium", "high"]
                    && schema.default_val == "high"
        ));
        assert!(matches!(
            &flash_lite_thinking.schema,
            SettingSchema::Enumeration(schema)
                if schema.allowed == vec!["minimal", "low", "medium", "high"]
                    && schema.default_val == "minimal"
        ));
    }

    #[test]
    fn message_to_gemini_includes_function_response_name_and_id() {
        let message = Message {
            role: "tool".into(),
            parts: vec![MessagePart::ToolResult(ur::extension::types::ToolResult {
                tool_call_id: "call-1".into(),
                tool_name: "lookup_weather".into(),
                content: "{\"temperature_f\":72}".into(),
            })],
        };

        let json = message_to_gemini(&message);

        assert_eq!(
            json["parts"][0]["functionResponse"]["name"],
            "lookup_weather"
        );
        assert_eq!(json["parts"][0]["functionResponse"]["id"], "call-1");
        assert_eq!(
            json["parts"][0]["functionResponse"]["response"]["temperature_f"],
            72
        );
    }

    #[test]
    fn build_request_body_preserves_ids_for_repeated_tool_names() {
        let messages = vec![
            Message {
                role: "user".into(),
                parts: vec![MessagePart::Text("Weather in Austin and Dallas?".into())],
            },
            Message {
                role: "tool".into(),
                parts: vec![MessagePart::ToolResult(ur::extension::types::ToolResult {
                    tool_call_id: "call-austin".into(),
                    tool_name: "lookup_weather".into(),
                    content: "{\"city\":\"Austin\"}".into(),
                })],
            },
            Message {
                role: "tool".into(),
                parts: vec![MessagePart::ToolResult(ur::extension::types::ToolResult {
                    tool_call_id: "call-dallas".into(),
                    tool_name: "lookup_weather".into(),
                    content: "{\"city\":\"Dallas\"}".into(),
                })],
            },
        ];

        let body = build_request_body(&messages, &[], &[]);
        let json: serde_json::Value = serde_json::from_str(&body).expect("request body JSON");

        assert_eq!(
            json["contents"][1]["parts"][0]["functionResponse"]["id"],
            "call-austin"
        );
        assert_eq!(
            json["contents"][2]["parts"][0]["functionResponse"]["id"],
            "call-dallas"
        );
    }

    #[test]
    fn build_request_body_encodes_thinking_level_and_ignores_temperature() {
        let messages = vec![Message {
            role: "user".into(),
            parts: vec![MessagePart::Text("Hello".into())],
        }];
        let settings = vec![
            ConfigSetting {
                key: "thinking_level".into(),
                value: ur::extension::types::SettingValue::Enumeration("low".into()),
            },
            ConfigSetting {
                key: "max_output_tokens".into(),
                value: ur::extension::types::SettingValue::Integer(1024),
            },
            ConfigSetting {
                key: "temperature".into(),
                value: ur::extension::types::SettingValue::Integer(150),
            },
        ];

        let body = build_request_body(&messages, &settings, &[]);
        let json: serde_json::Value = serde_json::from_str(&body).expect("request body JSON");

        assert_eq!(json["generationConfig"]["maxOutputTokens"], 1024);
        assert_eq!(
            json["generationConfig"]["thinkingConfig"]["thinkingLevel"],
            "low"
        );
        assert!(json["generationConfig"].get("temperature").is_none());
    }
}
