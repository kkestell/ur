//! Google Gemini LLM provider.
//!
//! Native port of the `llm-google` WASM extension. Uses `reqwest` for HTTP
//! and parses SSE events from the Gemini streaming API.

use anyhow::{Context, Result, bail};
use futures_util::StreamExt;
use reqwest::Client;

use super::LlmProvider;
use crate::types::{
    Completion, CompletionChunk, ConfigSetting, Message, MessagePart, ModelDescriptor,
    SettingDescriptor, SettingEnum, SettingInteger, SettingSchema, SettingString, SettingValue,
    TextPart, ToolCall, ToolChoice, ToolDescriptor, Usage,
};

// ── Constants ───────────────────────────────────────────────────────

const GEMINI_3_FLASH_PREVIEW: &str = "gemini-3-flash-preview";
const GEMINI_3_1_PRO_PREVIEW: &str = "gemini-3.1-pro-preview";
const GEMINI_3_1_FLASH_LITE_PREVIEW: &str = "gemini-3.1-flash-lite-preview";

/// Gemini 3.1 text models advertise up to 64k output tokens.
const GOOGLE_MAX_OUTPUT_TOKENS: i64 = 65_536;
const GOOGLE_DEFAULT_MAX_OUTPUT_TOKENS: i64 = 8_192;

const FLASH_THINKING_LEVELS: &[&str] = &["minimal", "low", "medium", "high"];
const PRO_THINKING_LEVELS: &[&str] = &["low", "medium", "high"];
const FLASH_LITE_THINKING_LEVELS: &[&str] = &["minimal", "low", "medium", "high"];

const GOOGLE_MODELS: &[ModelMeta] = &[
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

// ── Model metadata ──────────────────────────────────────────────────

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
    }
}

/// Builds the per-model settings descriptors (max tokens, thinking, readonly metadata).
fn model_settings_descriptors(meta: &ModelMeta) -> Vec<SettingDescriptor> {
    let id = meta.id;
    vec![
        SettingDescriptor {
            key: format!("{id}.max_output_tokens"),
            name: "Max Output Tokens".into(),
            description: "Maximum number of tokens to generate".into(),
            schema: SettingSchema::Integer(SettingInteger {
                min: 1,
                max: GOOGLE_MAX_OUTPUT_TOKENS,
                default_val: GOOGLE_DEFAULT_MAX_OUTPUT_TOKENS,
            }),
            secret: false,
            readonly: false,
        },
        SettingDescriptor {
            key: format!("{id}.thinking_level"),
            name: "Thinking Level".into(),
            description: "Relative reasoning depth".into(),
            schema: SettingSchema::Enumeration(SettingEnum {
                allowed: meta
                    .thinking_levels
                    .iter()
                    .map(|level| (*level).to_owned())
                    .collect(),
                default_val: meta.default_thinking_level.into(),
            }),
            secret: false,
            readonly: false,
        },
        SettingDescriptor {
            key: format!("{id}.context_window_in"),
            name: "Context Window (input)".into(),
            description: "Maximum input tokens".into(),
            schema: SettingSchema::Integer(SettingInteger {
                min: 0,
                max: i64::from(meta.context_window_in),
                default_val: i64::from(meta.context_window_in),
            }),
            secret: false,
            readonly: true,
        },
        SettingDescriptor {
            key: format!("{id}.context_window_out"),
            name: "Context Window (output)".into(),
            description: "Maximum output tokens".into(),
            schema: SettingSchema::Integer(SettingInteger {
                min: 0,
                max: i64::from(meta.context_window_out),
                default_val: i64::from(meta.context_window_out),
            }),
            secret: false,
            readonly: true,
        },
        SettingDescriptor {
            key: format!("{id}.knowledge_cutoff"),
            name: "Knowledge Cutoff".into(),
            description: "Training data cutoff date".into(),
            schema: SettingSchema::String(SettingString {
                default_val: meta.knowledge_cutoff.into(),
            }),
            secret: false,
            readonly: true,
        },
        SettingDescriptor {
            key: format!("{id}.cost_in"),
            name: "Input Cost".into(),
            description: "Millidollars per million input tokens".into(),
            schema: SettingSchema::Integer(SettingInteger {
                min: 0,
                max: i64::from(meta.cost_in),
                default_val: i64::from(meta.cost_in),
            }),
            secret: false,
            readonly: true,
        },
        SettingDescriptor {
            key: format!("{id}.cost_out"),
            name: "Output Cost".into(),
            description: "Millidollars per million output tokens".into(),
            schema: SettingSchema::Integer(SettingInteger {
                min: 0,
                max: i64::from(meta.cost_out),
                default_val: i64::from(meta.cost_out),
            }),
            secret: false,
            readonly: true,
        },
    ]
}

// ── Provider ────────────────────────────────────────────────────────

/// Native Google Gemini LLM provider.
#[derive(Debug)]
pub struct GoogleProvider {
    api_key: String,
    client: Client,
}

impl GoogleProvider {
    #[must_use]
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: Client::new(),
        }
    }
}

impl LlmProvider for GoogleProvider {
    fn provider_id(&self) -> &'static str {
        "google"
    }

    fn list_models(&self) -> Vec<ModelDescriptor> {
        GOOGLE_MODELS.iter().map(model_descriptor).collect()
    }

    fn list_settings(&self) -> Vec<SettingDescriptor> {
        let mut settings = vec![SettingDescriptor {
            key: "api_key".into(),
            name: "API Key".into(),
            description: "Google AI API key".into(),
            schema: SettingSchema::String(SettingString {
                default_val: String::new(),
            }),
            secret: true,
            readonly: false,
        }];

        for meta in GOOGLE_MODELS {
            settings.extend(model_settings_descriptors(meta));
        }

        settings
    }

    fn complete(
        &self,
        messages: &[Message],
        model_id: &str,
        settings: &[ConfigSetting],
        tools: &[ToolDescriptor],
        tool_choice: Option<&ToolChoice>,
        on_chunk: &mut dyn FnMut(CompletionChunk),
    ) -> Result<Completion> {
        let body = build_request_body(messages, settings, tools, tool_choice);
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{model_id}:streamGenerateContent?alt=sse"
        );

        // Run the async streaming request on the current tokio runtime.
        let handle = tokio::runtime::Handle::current();
        handle.block_on(self.stream_completion(&url, &body, on_chunk))
    }
}

impl GoogleProvider {
    async fn stream_completion(
        &self,
        url: &str,
        body: &str,
        on_chunk: &mut dyn FnMut(CompletionChunk),
    ) -> Result<Completion> {
        let response = self
            .client
            .post(url)
            .header("x-goog-api-key", &self.api_key)
            .header("content-type", "application/json")
            .body(body.to_owned())
            .send()
            .await
            .context("failed to send request to Gemini API")?;

        let status = response.status();
        if !status.is_success() {
            let err_text = response
                .text()
                .await
                .unwrap_or_else(|_| String::from("<unreadable>"));
            bail!("Gemini API error: HTTP {status}: {err_text}");
        }

        let mut stream = response.bytes_stream();
        let mut sse_buf = String::new();
        let mut all_parts: Vec<MessagePart> = Vec::new();
        let mut last_usage: Option<Usage> = None;

        while let Some(chunk_result) = stream.next().await {
            let bytes = chunk_result.context("error reading response stream")?;
            sse_buf.push_str(&String::from_utf8_lossy(&bytes));

            // Parse as many complete SSE events as we can from the buffer.
            while let Some((consumed, event_text)) = next_complete_sse_event(&sse_buf) {
                sse_buf.drain(..consumed);

                match parse_sse_event(&event_text) {
                    Ok(Some(chunk)) => {
                        accumulate_parts(&mut all_parts, &chunk.delta_parts);
                        if chunk.usage.is_some() {
                            last_usage.clone_from(&chunk.usage);
                        }
                        on_chunk(chunk);
                    }
                    Ok(None) => {}
                    Err(error) => {
                        tracing::warn!("skipping malformed SSE event: {error}");
                    }
                }
            }
        }

        // Try to parse any trailing data remaining in the buffer.
        if !sse_buf.is_empty()
            && let Some((consumed, event_text)) = next_complete_sse_event(&sse_buf)
        {
            sse_buf.drain(..consumed);
            if let Ok(Some(chunk)) = parse_sse_event(&event_text) {
                accumulate_parts(&mut all_parts, &chunk.delta_parts);
                if chunk.usage.is_some() {
                    last_usage.clone_from(&chunk.usage);
                }
                on_chunk(chunk);
            }
        }

        Ok(Completion {
            message: Message {
                role: "assistant".into(),
                parts: merge_text_parts(all_parts),
            },
            usage: last_usage,
        })
    }
}

// ── Accumulation helpers ────────────────────────────────────────────

/// Appends delta parts to the running accumulator.
fn accumulate_parts(all: &mut Vec<MessagePart>, delta: &[MessagePart]) {
    for part in delta {
        all.push(part.clone());
    }
}

/// Merges consecutive text parts into single text parts.
fn merge_text_parts(parts: Vec<MessagePart>) -> Vec<MessagePart> {
    let mut merged: Vec<MessagePart> = Vec::new();
    for part in parts {
        match (&part, merged.last_mut()) {
            (MessagePart::Text(new_text), Some(MessagePart::Text(existing))) => {
                existing.text.push_str(&new_text.text);
            }
            _ => merged.push(part),
        }
    }
    merged
}

// ── SSE parsing ─────────────────────────────────────────────────────

/// Tries to extract a complete SSE event from the buffer. Returns the
/// number of bytes consumed and the event payload (without the trailing
/// blank-line delimiter).
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

/// Parses an SSE event block into a `CompletionChunk`, returning `None`
/// for events that carry no data (comments, `[DONE]` sentinel).
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
        .and_then(|c| c.as_array())
        .ok_or_else(|| "missing SSE candidates".to_string())?;
    let candidate = candidates
        .first()
        .ok_or_else(|| "missing SSE candidate".to_string())?;
    let content = candidate
        .get("content")
        .ok_or_else(|| "missing SSE content".to_string())?;
    let parts = content
        .get("parts")
        .and_then(|p| p.as_array())
        .ok_or_else(|| "missing SSE parts".to_string())?;

    let mut delta_parts = Vec::new();

    for part in parts {
        if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
            delta_parts.push(MessagePart::Text(TextPart {
                text: text.to_string(),
            }));
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

    let usage = response.get("usageMetadata").and_then(parse_usage_metadata);

    Ok(CompletionChunk { delta_parts, usage })
}

// ── Request body construction ───────────────────────────────────────

fn build_request_body(
    messages: &[Message],
    settings: &[ConfigSetting],
    tools: &[ToolDescriptor],
    tool_choice: Option<&ToolChoice>,
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

        // Map tool_choice to Gemini toolConfig.functionCallingConfig.
        if let Some(tc) = tool_choice {
            let config = match tc {
                ToolChoice::Auto => serde_json::json!({"mode": "AUTO"}),
                ToolChoice::None => serde_json::json!({"mode": "NONE"}),
                ToolChoice::Required => serde_json::json!({"mode": "ANY"}),
                ToolChoice::Specific(name) => serde_json::json!({
                    "mode": "ANY",
                    "allowedFunctionNames": [name]
                }),
            };
            body.insert(
                "toolConfig".into(),
                serde_json::json!({"functionCallingConfig": config}),
            );
        }
    }

    // Generation config from settings.
    let mut gen_config = serde_json::Map::new();
    let mut thinking_config = serde_json::Map::new();
    for s in settings {
        match s.key.as_str() {
            "max_output_tokens" => {
                if let SettingValue::Integer(v) = &s.value {
                    gen_config.insert("maxOutputTokens".into(), serde_json::json!(v));
                }
            }
            "thinking_level" => {
                if let SettingValue::Enumeration(level) = &s.value {
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
            MessagePart::Text(t) => Some(t.text.as_str()),
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
        prompt_tokens: u32::try_from(usage_metadata.get("promptTokenCount")?.as_u64()?).ok()?,
        completion_tokens: u32::try_from(usage_metadata.get("candidatesTokenCount")?.as_u64()?)
            .ok()?,
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
            MessagePart::Text(t) => serde_json::json!({ "text": t.text }),
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

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ToolResult;

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

    #[test]
    fn next_complete_sse_event_keeps_partial_json_buffered() {
        let buf = "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"hel";
        assert!(next_complete_sse_event(buf).is_none());
    }

    #[test]
    fn next_complete_sse_event_requires_blank_line_terminator() {
        let json = text_chunk_json("Hello");
        let buf = format!("data: {json}");
        assert!(next_complete_sse_event(&buf).is_none());
    }

    #[test]
    fn next_complete_sse_event_emits_events_one_at_a_time() {
        let first = text_chunk_json("Hello");
        let second = text_chunk_json(" world");
        let buf = format!("data: {first}\n\ndata: {second}\n\n");

        let (consumed, event1) = next_complete_sse_event(&buf).expect("first event");
        let chunk1 = parse_sse_event(&event1)
            .expect("parse first")
            .expect("first chunk");
        assert!(matches!(
            &chunk1.delta_parts[0],
            MessagePart::Text(t) if t.text == "Hello"
        ));

        let (_, event2) = next_complete_sse_event(&buf[consumed..]).expect("second event");
        let chunk2 = parse_sse_event(&event2)
            .expect("parse second")
            .expect("second chunk");
        assert!(matches!(
            &chunk2.delta_parts[0],
            MessagePart::Text(t) if t.text == " world"
        ));
    }

    #[test]
    fn next_complete_sse_event_accepts_crlf_delimiters() {
        let json = text_chunk_json("Hello");
        let buf = format!("data: {json}\r\n\r\n");

        let (_, event) = next_complete_sse_event(&buf).expect("CRLF event");
        let chunk = parse_sse_event(&event).expect("parse").expect("chunk");
        assert!(matches!(
            &chunk.delta_parts[0],
            MessagePart::Text(t) if t.text == "Hello"
        ));
    }

    #[test]
    fn list_models_advertises_current_gemini_3_text_models() {
        let provider = GoogleProvider::new("test-key".into());
        let models = provider.list_models();
        let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();

        assert_eq!(
            ids,
            vec![
                "gemini-3-flash-preview",
                "gemini-3.1-pro-preview",
                "gemini-3.1-flash-lite-preview",
            ]
        );
        assert_eq!(
            models.iter().find(|m| m.is_default).map(|m| m.id.as_str()),
            Some("gemini-3-flash-preview")
        );
    }

    #[test]
    fn list_settings_exposes_model_specific_thinking_levels() {
        let provider = GoogleProvider::new("test-key".into());
        let settings = provider.list_settings();

        // api_key is first, then per-model settings
        assert!(settings.iter().any(|s| s.key == "api_key" && s.secret));

        let flash_thinking = settings
            .iter()
            .find(|s| s.key == "gemini-3-flash-preview.thinking_level")
            .expect("flash thinking setting");
        let pro_thinking = settings
            .iter()
            .find(|s| s.key == "gemini-3.1-pro-preview.thinking_level")
            .expect("pro thinking setting");
        let flash_lite_thinking = settings
            .iter()
            .find(|s| s.key == "gemini-3.1-flash-lite-preview.thinking_level")
            .expect("flash-lite thinking setting");

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

        // Readonly metadata
        let flash_ctx = settings
            .iter()
            .find(|s| s.key == "gemini-3-flash-preview.context_window_in")
            .expect("flash context window setting");
        assert!(flash_ctx.readonly);
    }

    #[test]
    fn message_to_gemini_includes_function_response_name_and_id() {
        let message = Message {
            role: "tool".into(),
            parts: vec![MessagePart::ToolResult(ToolResult {
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
                parts: vec![MessagePart::Text(TextPart {
                    text: "Weather in Austin and Dallas?".into(),
                })],
            },
            Message {
                role: "tool".into(),
                parts: vec![MessagePart::ToolResult(ToolResult {
                    tool_call_id: "call-austin".into(),
                    tool_name: "lookup_weather".into(),
                    content: "{\"city\":\"Austin\"}".into(),
                })],
            },
            Message {
                role: "tool".into(),
                parts: vec![MessagePart::ToolResult(ToolResult {
                    tool_call_id: "call-dallas".into(),
                    tool_name: "lookup_weather".into(),
                    content: "{\"city\":\"Dallas\"}".into(),
                })],
            },
        ];

        let body = build_request_body(&messages, &[], &[], None);
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
            parts: vec![MessagePart::Text(TextPart {
                text: "Hello".into(),
            })],
        }];
        let settings = vec![
            ConfigSetting {
                key: "thinking_level".into(),
                value: SettingValue::Enumeration("low".into()),
            },
            ConfigSetting {
                key: "max_output_tokens".into(),
                value: SettingValue::Integer(1024),
            },
            ConfigSetting {
                key: "temperature".into(),
                value: SettingValue::Integer(150),
            },
        ];

        let body = build_request_body(&messages, &settings, &[], None);
        let json: serde_json::Value = serde_json::from_str(&body).expect("request body JSON");

        assert_eq!(json["generationConfig"]["maxOutputTokens"], 1024);
        assert_eq!(
            json["generationConfig"]["thinkingConfig"]["thinkingLevel"],
            "low"
        );
        assert!(json["generationConfig"].get("temperature").is_none());
    }

    #[test]
    fn merge_text_parts_concatenates_consecutive_text() {
        let parts = vec![
            MessagePart::Text(TextPart {
                text: "Hello".into(),
            }),
            MessagePart::Text(TextPart {
                text: " world".into(),
            }),
            MessagePart::ToolCall(ToolCall {
                id: "1".into(),
                name: "test".into(),
                arguments_json: "{}".into(),
                provider_metadata_json: String::new(),
            }),
            MessagePart::Text(TextPart {
                text: "after".into(),
            }),
        ];

        let merged = merge_text_parts(parts);
        assert_eq!(merged.len(), 3);
        assert!(matches!(&merged[0], MessagePart::Text(t) if t.text == "Hello world"));
        assert!(matches!(&merged[1], MessagePart::ToolCall(_)));
        assert!(matches!(&merged[2], MessagePart::Text(t) if t.text == "after"));
    }

    #[test]
    fn parse_sse_chunk_extracts_usage_metadata() {
        let json = serde_json::json!({
            "candidates": [{
                "content": {
                    "parts": [{"text": "hi"}]
                }
            }],
            "usageMetadata": {
                "promptTokenCount": 10,
                "candidatesTokenCount": 5
            }
        })
        .to_string();

        let chunk = parse_sse_chunk(&json).expect("valid chunk");
        let usage = chunk.usage.expect("usage present");
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.completion_tokens, 5);
    }

    #[test]
    fn parse_sse_chunk_extracts_tool_call_with_metadata() {
        let json = serde_json::json!({
            "candidates": [{
                "content": {
                    "parts": [{
                        "functionCall": {
                            "name": "get_weather",
                            "id": "call-42",
                            "args": {"city": "Austin"}
                        },
                        "thoughtSignature": "abc123"
                    }]
                }
            }]
        })
        .to_string();

        let chunk = parse_sse_chunk(&json).expect("valid chunk");
        assert_eq!(chunk.delta_parts.len(), 1);
        match &chunk.delta_parts[0] {
            MessagePart::ToolCall(tc) => {
                assert_eq!(tc.name, "get_weather");
                assert_eq!(tc.id, "call-42");
                assert!(tc.arguments_json.contains("Austin"));
                assert!(tc.provider_metadata_json.contains("abc123"));
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }
}
