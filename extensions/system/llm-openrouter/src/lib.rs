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
    SettingBoolean, SettingDescriptor, SettingInteger, SettingNumber, SettingSchema, SettingValue,
    ToolCall, ToolDescriptor, Usage,
};
use wasi::http::outgoing_handler;
use wasi::http::types::{Fields, IncomingBody, Method, OutgoingBody, OutgoingRequest, Scheme};
use wasi::io::streams::StreamError;

// ── Thread-local state ──────────────────────────────────────────────

thread_local! {
    static API_KEY: RefCell<Option<String>> = const { RefCell::new(None) };
    static CACHED_CATALOG: RefCell<Option<Vec<CatalogModel>>> = const { RefCell::new(None) };
}

fn get_api_key() -> Result<String, String> {
    API_KEY.with(|k| {
        k.borrow().clone().ok_or_else(|| {
            "No API key for provider 'openrouter'. Set one with: ur config set-key openrouter"
                .into()
        })
    })
}

struct LlmOpenRouter;

// ── Extension lifecycle ─────────────────────────────────────────────

impl ExtGuest for LlmOpenRouter {
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
        "llm-openrouter".into()
    }

    fn name() -> String {
        "OpenRouter".into()
    }

    fn list_tools() -> Vec<ToolDescriptor> {
        vec![]
    }
}

// ── LLM provider ────────────────────────────────────────────────────

impl LlmGuest for LlmOpenRouter {
    fn provider_id() -> String {
        "openrouter".into()
    }

    fn list_models() -> Vec<ModelDescriptor> {
        let catalog = match fetch_catalog() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("openrouter: failed to fetch catalog: {e}");
                return vec![];
            }
        };

        let mut descriptors: Vec<ModelDescriptor> =
            catalog.iter().map(catalog_model_to_descriptor).collect();

        // Mark exactly one model as default.
        let default_idx = pick_default_index(&descriptors);
        if let Some(idx) = default_idx {
            descriptors[idx].is_default = true;
        }

        CACHED_CATALOG.with(|c| *c.borrow_mut() = Some(catalog));

        descriptors
    }

    fn complete(
        messages: Vec<Message>,
        model: String,
        settings: Vec<ConfigSetting>,
        tools: Vec<ToolDescriptor>,
    ) -> Result<Completion, String> {
        let api_key = get_api_key()?;
        let body = build_request_body(&model, &messages, &settings, &tools, false);
        let response_bytes = http_post(&api_key, "/api/v1/chat/completions", &body)?;
        let response_str =
            String::from_utf8(response_bytes).map_err(|e| format!("invalid UTF-8: {e}"))?;
        parse_chat_response(&response_str)
    }
}

// ── Streaming provider ──────────────────────────────────────────────

struct OpenRouterStream {
    inner: RefCell<StreamState>,
}

struct StreamState {
    buffer: String,
    pos: usize,
    done: bool,
    body_stream: Option<wasi::io::streams::InputStream>,
    _incoming_body: Option<IncomingBody>,
    // Accumulate tool call deltas across chunks.
    pending_tool_calls: Vec<PendingToolCall>,
}

#[derive(Clone)]
struct PendingToolCall {
    index: u32,
    id: String,
    name: String,
    arguments: String,
}

impl LlmStreamingGuest for LlmOpenRouter {
    type CompletionStream = OpenRouterStream;

    fn complete_streaming(
        messages: Vec<Message>,
        model: String,
        settings: Vec<ConfigSetting>,
        tools: Vec<ToolDescriptor>,
    ) -> Result<CompletionStream, String> {
        let api_key = get_api_key()?;
        let body = build_request_body(&model, &messages, &settings, &tools, true);

        let (incoming_body, body_stream) =
            http_post_streaming(&api_key, "/api/v1/chat/completions", &body)?;

        Ok(CompletionStream::new(OpenRouterStream {
            inner: RefCell::new(StreamState {
                buffer: String::new(),
                pos: 0,
                done: false,
                body_stream: Some(body_stream),
                _incoming_body: Some(incoming_body),
                pending_tool_calls: Vec::new(),
            }),
        }))
    }
}

impl GuestCompletionStream for OpenRouterStream {
    fn next(&self) -> Option<CompletionChunk> {
        let mut state = self.inner.borrow_mut();

        if state.done {
            return None;
        }

        loop {
            if let Some(chunk) = try_parse_sse_event(&mut state) {
                return Some(chunk);
            }

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

        match parse_sse_event(&event, &mut state.pending_tool_calls) {
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

fn parse_sse_event(
    event: &str,
    pending_tool_calls: &mut Vec<PendingToolCall>,
) -> Result<Option<CompletionChunk>, String> {
    let mut data_lines = Vec::new();

    for raw_line in event.lines() {
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        // Skip SSE comments (keepalive lines like ": OPENROUTER PROCESSING").
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
        // Emit any remaining pending tool calls as a final chunk.
        if !pending_tool_calls.is_empty() {
            let parts = pending_tool_calls
                .drain(..)
                .map(|tc| {
                    MessagePart::ToolCall(ToolCall {
                        id: tc.id,
                        name: tc.name,
                        arguments_json: tc.arguments,
                        provider_metadata_json: String::new(),
                    })
                })
                .collect();
            return Ok(Some(CompletionChunk {
                delta_parts: parts,
                finish_reason: Some("tool_calls".into()),
                usage: None,
            }));
        }
        return Ok(None);
    }

    parse_sse_chunk(&payload, pending_tool_calls).map(Some)
}

fn parse_sse_chunk(
    json_str: &str,
    pending_tool_calls: &mut Vec<PendingToolCall>,
) -> Result<CompletionChunk, String> {
    let response: serde_json::Value =
        serde_json::from_str(json_str).map_err(|e| format!("invalid SSE JSON: {e}"))?;

    // Check for mid-stream error events.
    if let Some(error) = response.get("error") {
        let msg = error
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown error");
        return Err(format!("OpenRouter streaming error: {msg}"));
    }

    let choices = response
        .get("choices")
        .and_then(|c| c.as_array())
        .ok_or_else(|| "missing choices in SSE chunk".to_string())?;
    let choice = choices
        .first()
        .ok_or_else(|| "empty choices in SSE chunk".to_string())?;
    let delta = choice
        .get("delta")
        .ok_or_else(|| "missing delta in SSE chunk".to_string())?;

    let mut delta_parts = Vec::new();

    // Text content.
    if let Some(content) = delta.get("content").and_then(|c| c.as_str())
        && !content.is_empty()
    {
        delta_parts.push(MessagePart::Text(content.to_string()));
    }

    // Tool call deltas — accumulate across chunks.
    if let Some(tool_calls) = delta.get("tool_calls").and_then(|t| t.as_array()) {
        for tc_delta in tool_calls {
            let index = u32::try_from(
                tc_delta
                    .get("index")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0),
            )
            .unwrap_or(0);

            // Find or create the pending tool call for this index.
            let pending = if let Some(p) = pending_tool_calls.iter_mut().find(|p| p.index == index)
            {
                p
            } else {
                pending_tool_calls.push(PendingToolCall {
                    index,
                    id: String::new(),
                    name: String::new(),
                    arguments: String::new(),
                });
                pending_tool_calls.last_mut().unwrap()
            };

            if let Some(id) = tc_delta.get("id").and_then(|i| i.as_str()) {
                pending.id = id.to_string();
            }
            if let Some(function) = tc_delta.get("function") {
                if let Some(name) = function.get("name").and_then(|n| n.as_str()) {
                    pending.name.push_str(name);
                }
                if let Some(args) = function.get("arguments").and_then(|a| a.as_str()) {
                    pending.arguments.push_str(args);
                }
            }
        }
    }

    let finish_reason = choice
        .get("finish_reason")
        .and_then(|r| r.as_str())
        .map(String::from);

    // When finish_reason is set and we have pending tool calls, emit them.
    if finish_reason.is_some() && !pending_tool_calls.is_empty() {
        let parts: Vec<MessagePart> = pending_tool_calls
            .drain(..)
            .map(|tc| {
                MessagePart::ToolCall(ToolCall {
                    id: tc.id,
                    name: tc.name,
                    arguments_json: tc.arguments,
                    provider_metadata_json: String::new(),
                })
            })
            .collect();
        delta_parts.extend(parts);
    }

    let usage = response.get("usage").and_then(parse_usage);

    Ok(CompletionChunk {
        delta_parts,
        finish_reason,
        usage,
    })
}

// ── Catalog fetch ───────────────────────────────────────────────────

/// Raw model entry from the `OpenRouter` `GET /api/v1/models` response.
#[derive(serde::Deserialize, Clone)]
struct CatalogModel {
    id: String,
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    context_length: Option<u64>,
    #[serde(default)]
    top_provider: Option<TopProvider>,
    #[serde(default)]
    pricing: Option<Pricing>,
    #[serde(default)]
    architecture: Option<Architecture>,
    #[serde(default)]
    supported_parameters: Option<Vec<String>>,
}

#[derive(serde::Deserialize, Clone)]
struct TopProvider {
    #[serde(default)]
    max_completion_tokens: Option<u64>,
}

#[derive(serde::Deserialize, Clone)]
struct Pricing {
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    completion: Option<String>,
}

#[derive(serde::Deserialize, Clone)]
struct Architecture {
    #[serde(default)]
    input_modalities: Option<Vec<String>>,
    #[serde(default)]
    output_modalities: Option<Vec<String>>,
}

#[derive(serde::Deserialize)]
struct CatalogResponse {
    data: Vec<CatalogModel>,
}

fn fetch_catalog() -> Result<Vec<CatalogModel>, String> {
    let api_key = get_api_key()?;
    let bytes = http_get(
        &api_key,
        "/api/v1/models?supported_parameters=tools&output_modalities=text",
    )?;
    let body = String::from_utf8(bytes).map_err(|e| format!("invalid UTF-8: {e}"))?;
    let response: CatalogResponse =
        serde_json::from_str(&body).map_err(|e| format!("failed to parse catalog: {e}"))?;

    let mut filtered: Vec<CatalogModel> =
        response.data.into_iter().filter(is_usable_model).collect();

    filtered.sort_by(|a, b| a.id.cmp(&b.id));

    Ok(filtered)
}

fn is_usable_model(m: &CatalogModel) -> bool {
    if m.id.is_empty() || m.name.is_empty() {
        return false;
    }

    // Must support tools.
    let supports_tools = m
        .supported_parameters
        .as_ref()
        .is_some_and(|params| params.iter().any(|p| p == "tools"));
    if !supports_tools {
        return false;
    }

    // Must support text input and output.
    if let Some(arch) = &m.architecture {
        let text_in = arch
            .input_modalities
            .as_ref()
            .is_some_and(|mods| mods.iter().any(|m| m == "text"));
        let text_out = arch
            .output_modalities
            .as_ref()
            .is_some_and(|mods| mods.iter().any(|m| m == "text"));
        if !text_in || !text_out {
            return false;
        }
    } else {
        return false;
    }

    // Must have a context window.
    if m.context_length.unwrap_or(0) == 0 {
        return false;
    }

    true
}

// ── Catalog → ModelDescriptor mapping ───────────────────────────────

fn catalog_model_to_descriptor(m: &CatalogModel) -> ModelDescriptor {
    let context_in = u32::try_from(m.context_length.unwrap_or(0)).unwrap_or(u32::MAX);
    let context_out = m
        .top_provider
        .as_ref()
        .and_then(|tp| tp.max_completion_tokens)
        .map_or(4096, |v| u32::try_from(v).unwrap_or(u32::MAX));

    let (cost_in, cost_out) = convert_pricing(m);

    let settings = build_model_settings(m, context_out);

    ModelDescriptor {
        id: m.id.clone(),
        name: m.name.clone(),
        description: m.description.clone(),
        is_default: false,
        settings,
        context_window_in: context_in,
        context_window_out: context_out,
        knowledge_cutoff: "unknown".into(),
        cost_in,
        cost_out,
    }
}

/// Convert pricing (dollars per token as decimal strings) to
/// millidollars per million tokens.
fn convert_pricing(m: &CatalogModel) -> (u32, u32) {
    let Some(pricing) = &m.pricing else {
        return (0, 0);
    };

    let cost_in = pricing
        .prompt
        .as_deref()
        .and_then(|s| s.parse::<f64>().ok())
        .map_or(0, dollars_per_token_to_millidollars_per_million);

    let cost_out = pricing
        .completion
        .as_deref()
        .and_then(|s| s.parse::<f64>().ok())
        .map_or(0, dollars_per_token_to_millidollars_per_million);

    (cost_in, cost_out)
}

/// Converts dollars-per-token to millidollars-per-million-tokens.
///
/// `ur` stores costs as `u32` millidollars per million tokens.
/// The catalog returns dollars per token as a decimal string.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "Value is bounds-checked before cast"
)]
fn dollars_per_token_to_millidollars_per_million(dollars_per_token: f64) -> u32 {
    let millidollars = dollars_per_token * 1e9;
    if millidollars < 0.0 {
        0
    } else if millidollars > f64::from(u32::MAX) {
        u32::MAX
    } else {
        millidollars.round() as u32
    }
}

fn build_model_settings(m: &CatalogModel, max_output: u32) -> Vec<SettingDescriptor> {
    let supported = m.supported_parameters.as_deref().unwrap_or(&[]);
    let mut settings = Vec::new();

    if has_param(supported, "max_tokens") || has_param(supported, "max_completion_tokens") {
        settings.push(SettingDescriptor {
            key: "max_output_tokens".into(),
            name: "Max Output Tokens".into(),
            description: "Maximum number of tokens to generate".into(),
            schema: SettingSchema::Integer(SettingInteger {
                min: 1,
                max: i64::from(max_output),
                default_val: i64::from(max_output.min(4096)),
            }),
        });
    }

    if has_param(supported, "temperature") {
        settings.push(SettingDescriptor {
            key: "temperature".into(),
            name: "Temperature".into(),
            description: "Sampling temperature (0.0 = deterministic, 2.0 = creative)".into(),
            schema: SettingSchema::Number(SettingNumber {
                min: 0.0,
                max: 2.0,
                default_val: 1.0,
            }),
        });
    }

    if has_param(supported, "top_p") {
        settings.push(SettingDescriptor {
            key: "top_p".into(),
            name: "Top P".into(),
            description: "Nucleus sampling threshold".into(),
            schema: SettingSchema::Number(SettingNumber {
                min: 0.0,
                max: 1.0,
                default_val: 1.0,
            }),
        });
    }

    if has_param(supported, "frequency_penalty") {
        settings.push(SettingDescriptor {
            key: "frequency_penalty".into(),
            name: "Frequency Penalty".into(),
            description: "Penalizes token frequency in output".into(),
            schema: SettingSchema::Number(SettingNumber {
                min: -2.0,
                max: 2.0,
                default_val: 0.0,
            }),
        });
    }

    if has_param(supported, "presence_penalty") {
        settings.push(SettingDescriptor {
            key: "presence_penalty".into(),
            name: "Presence Penalty".into(),
            description: "Penalizes token presence in output".into(),
            schema: SettingSchema::Number(SettingNumber {
                min: -2.0,
                max: 2.0,
                default_val: 0.0,
            }),
        });
    }

    if has_param(supported, "seed") {
        settings.push(SettingDescriptor {
            key: "seed".into(),
            name: "Seed".into(),
            description: "Random seed for deterministic generation".into(),
            schema: SettingSchema::Integer(SettingInteger {
                min: 0,
                max: i64::from(i32::MAX),
                default_val: 0,
            }),
        });
    }

    if has_param(supported, "parallel_tool_calls") {
        settings.push(SettingDescriptor {
            key: "parallel_tool_calls".into(),
            name: "Parallel Tool Calls".into(),
            description: "Allow the model to make multiple tool calls in parallel".into(),
            schema: SettingSchema::Boolean(SettingBoolean { default_val: true }),
        });
    }

    settings
}

fn has_param(supported: &[String], name: &str) -> bool {
    supported.iter().any(|p| p == name)
}

// ── Default model selection ─────────────────────────────────────────

/// Preferred default models, in order of preference.
const DEFAULT_CANDIDATES: &[&str] = &[
    "openai/gpt-4o-mini",
    "openai/gpt-4o",
    "anthropic/claude-sonnet-4",
    "google/gemini-2.5-flash",
];

fn pick_default_index(descriptors: &[ModelDescriptor]) -> Option<usize> {
    for candidate in DEFAULT_CANDIDATES {
        if let Some(idx) = descriptors.iter().position(|d| d.id == *candidate) {
            return Some(idx);
        }
    }
    // Fall back to the first model.
    if descriptors.is_empty() {
        None
    } else {
        Some(0)
    }
}

// ── Request body construction ───────────────────────────────────────

fn build_request_body(
    model: &str,
    messages: &[Message],
    settings: &[ConfigSetting],
    tools: &[ToolDescriptor],
    stream: bool,
) -> String {
    let mut body = serde_json::Map::new();

    body.insert("model".into(), serde_json::json!(model));
    body.insert("stream".into(), serde_json::json!(stream));

    // Build messages array.
    let msgs: Vec<serde_json::Value> = messages.iter().map(message_to_openai).collect();
    body.insert("messages".into(), serde_json::Value::Array(msgs));

    // Tools.
    if !tools.is_empty() {
        let tool_defs: Vec<serde_json::Value> = tools
            .iter()
            .map(|t| {
                let params: serde_json::Value = serde_json::from_str(&t.parameters_json_schema)
                    .unwrap_or(serde_json::json!({"type": "object"}));
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": params
                    }
                })
            })
            .collect();
        body.insert("tools".into(), serde_json::Value::Array(tool_defs));
    }

    // Settings.
    for s in settings {
        match s.key.as_str() {
            "max_output_tokens" => {
                if let SettingValue::Integer(v) = &s.value {
                    body.insert("max_tokens".into(), serde_json::json!(v));
                }
            }
            "temperature" => {
                if let SettingValue::Number(v) = &s.value {
                    body.insert("temperature".into(), serde_json::json!(v));
                }
            }
            "top_p" => {
                if let SettingValue::Number(v) = &s.value {
                    body.insert("top_p".into(), serde_json::json!(v));
                }
            }
            "frequency_penalty" => {
                if let SettingValue::Number(v) = &s.value {
                    body.insert("frequency_penalty".into(), serde_json::json!(v));
                }
            }
            "presence_penalty" => {
                if let SettingValue::Number(v) = &s.value {
                    body.insert("presence_penalty".into(), serde_json::json!(v));
                }
            }
            "seed" => {
                if let SettingValue::Integer(v) = &s.value {
                    body.insert("seed".into(), serde_json::json!(v));
                }
            }
            "parallel_tool_calls" => {
                if let SettingValue::Boolean(v) = &s.value {
                    body.insert("parallel_tool_calls".into(), serde_json::json!(v));
                }
            }
            _ => {}
        }
    }

    // Require parameters so settings/tool use are not silently ignored.
    body.insert(
        "provider".into(),
        serde_json::json!({"require_parameters": true}),
    );

    serde_json::to_string(&body).unwrap_or_default()
}

fn message_to_openai(msg: &Message) -> serde_json::Value {
    let role = match msg.role.as_str() {
        "tool" => "tool",
        "assistant" => "assistant",
        "system" => "system",
        _ => "user",
    };

    // Tool result messages.
    if role == "tool"
        && let Some(MessagePart::ToolResult(tr)) = msg.parts.first()
    {
        return serde_json::json!({
            "role": "tool",
            "tool_call_id": tr.tool_call_id,
            "content": tr.content,
        });
    }

    // Assistant messages with tool calls.
    if role == "assistant" {
        let tool_calls: Vec<&ToolCall> = msg
            .parts
            .iter()
            .filter_map(|p| match p {
                MessagePart::ToolCall(tc) => Some(tc),
                _ => None,
            })
            .collect();

        if !tool_calls.is_empty() {
            let text = extract_text(msg);
            let tc_json: Vec<serde_json::Value> = tool_calls
                .iter()
                .map(|tc| {
                    serde_json::json!({
                        "id": tc.id,
                        "type": "function",
                        "function": {
                            "name": tc.name,
                            "arguments": tc.arguments_json
                        }
                    })
                })
                .collect();

            let mut obj = serde_json::json!({
                "role": "assistant",
                "tool_calls": tc_json
            });
            if !text.is_empty() {
                obj["content"] = serde_json::Value::String(text);
            }
            return obj;
        }
    }

    // Regular text messages.
    let text = extract_text(msg);
    serde_json::json!({
        "role": role,
        "content": text
    })
}

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

// ── Response parsing ────────────────────────────────────────────────

fn parse_chat_response(body: &str) -> Result<Completion, String> {
    let response: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("failed to parse response: {e}"))?;

    if let Some(error) = response.get("error") {
        let msg = error
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown error");
        return Err(format!("OpenRouter API error: {msg}"));
    }

    let choices = response
        .get("choices")
        .and_then(|c| c.as_array())
        .ok_or("no choices in response")?;
    let choice = choices.first().ok_or("empty choices array")?;
    let message = choice.get("message").ok_or("no message in choice")?;

    let mut parts = Vec::new();

    if let Some(content) = message.get("content").and_then(|c| c.as_str())
        && !content.is_empty()
    {
        parts.push(MessagePart::Text(content.to_string()));
    }

    if let Some(tool_calls) = message.get("tool_calls").and_then(|t| t.as_array()) {
        for tc in tool_calls {
            let id = tc.get("id").and_then(|i| i.as_str()).unwrap_or("");
            let function = tc.get("function").unwrap_or(&serde_json::Value::Null);
            let name = function.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let args = function
                .get("arguments")
                .and_then(|a| a.as_str())
                .unwrap_or("{}");
            parts.push(MessagePart::ToolCall(ToolCall {
                id: id.into(),
                name: name.into(),
                arguments_json: args.into(),
                provider_metadata_json: String::new(),
            }));
        }
    }

    let usage = response.get("usage").and_then(parse_usage);

    Ok(Completion {
        message: Message {
            role: "assistant".into(),
            parts,
        },
        usage,
    })
}

fn parse_usage(usage: &serde_json::Value) -> Option<Usage> {
    Some(Usage {
        input_tokens: u32::try_from(usage.get("prompt_tokens")?.as_u64()?).ok()?,
        output_tokens: u32::try_from(usage.get("completion_tokens")?.as_u64()?).ok()?,
    })
}

// ── WASI HTTP helpers ───────────────────────────────────────────────

fn http_get(api_key: &str, path: &str) -> Result<Vec<u8>, String> {
    let headers = Fields::from_list(&[
        (
            "authorization".into(),
            format!("Bearer {api_key}").into_bytes(),
        ),
        ("content-type".into(), b"application/json".to_vec()),
    ])
    .map_err(|error| format!("failed to create headers: {error:?}"))?;

    let request = OutgoingRequest::new(headers);
    request
        .set_method(&Method::Get)
        .map_err(|()| "failed to set method")?;
    request
        .set_scheme(Some(&Scheme::Https))
        .map_err(|()| "failed to set scheme")?;
    request
        .set_authority(Some("openrouter.ai"))
        .map_err(|()| "failed to set authority")?;
    request
        .set_path_with_query(Some(path))
        .map_err(|()| "failed to set path")?;

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
            return Err(format!("OpenRouter API error: HTTP {status}: {err_text}"));
        }
        return Err(format!("OpenRouter API error: HTTP {status}"));
    }

    let incoming_body = response
        .consume()
        .map_err(|()| "failed to consume response body".to_string())?;
    let body_stream = incoming_body
        .stream()
        .map_err(|()| "failed to get response stream".to_string())?;

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

fn http_post(api_key: &str, path: &str, body: &str) -> Result<Vec<u8>, String> {
    let (incoming_body, body_stream) = http_post_streaming(api_key, path, body)?;

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
        (
            "authorization".into(),
            format!("Bearer {api_key}").into_bytes(),
        ),
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
        .set_authority(Some("openrouter.ai"))
        .map_err(|()| "failed to set authority")?;
    request
        .set_path_with_query(Some(path))
        .map_err(|()| "failed to set path")?;

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
            return Err(format!("OpenRouter API error: HTTP {status}: {err_text}"));
        }
        return Err(format!("OpenRouter API error: HTTP {status}"));
    }

    let incoming_body = response
        .consume()
        .map_err(|()| "failed to consume response body".to_string())?;
    let body_stream = incoming_body
        .stream()
        .map_err(|()| "failed to get response stream".to_string())?;

    Ok((incoming_body, body_stream))
}

export!(LlmOpenRouter);

#[cfg(test)]
mod tests {
    use super::*;

    // ── Pricing conversion tests ────────────────────────────────────

    #[test]
    fn pricing_conversion_typical_values() {
        // OpenRouter returns "0.00000015" for $0.15/1M tokens.
        // millidollars_per_million = 0.00000015 * 1e9 = 150
        assert_eq!(
            dollars_per_token_to_millidollars_per_million(0.000_000_15),
            150
        );
    }

    #[test]
    fn pricing_conversion_zero() {
        assert_eq!(dollars_per_token_to_millidollars_per_million(0.0), 0);
    }

    #[test]
    fn pricing_conversion_negative_clamps_to_zero() {
        assert_eq!(dollars_per_token_to_millidollars_per_million(-1.0), 0);
    }

    #[test]
    fn pricing_conversion_expensive_model() {
        // $60/1M tokens = $0.00006/token
        // millidollars_per_million = 0.00006 * 1e9 = 60_000
        assert_eq!(
            dollars_per_token_to_millidollars_per_million(0.000_06),
            60_000
        );
    }

    // ── Catalog filtering tests ─────────────────────────────────────

    fn base_catalog_model() -> CatalogModel {
        CatalogModel {
            id: "test/model".into(),
            name: "Test Model".into(),
            description: "A test model".into(),
            context_length: Some(128_000),
            top_provider: Some(TopProvider {
                max_completion_tokens: Some(4096),
            }),
            pricing: Some(Pricing {
                prompt: Some("0.00000015".into()),
                completion: Some("0.0000006".into()),
            }),
            architecture: Some(Architecture {
                input_modalities: Some(vec!["text".into()]),
                output_modalities: Some(vec!["text".into()]),
            }),
            supported_parameters: Some(vec![
                "tools".into(),
                "temperature".into(),
                "max_tokens".into(),
            ]),
        }
    }

    #[test]
    fn is_usable_model_accepts_valid() {
        assert!(is_usable_model(&base_catalog_model()));
    }

    #[test]
    fn is_usable_model_rejects_empty_id() {
        let mut m = base_catalog_model();
        m.id = String::new();
        assert!(!is_usable_model(&m));
    }

    #[test]
    fn is_usable_model_rejects_missing_tools() {
        let mut m = base_catalog_model();
        m.supported_parameters = Some(vec!["temperature".into()]);
        assert!(!is_usable_model(&m));
    }

    #[test]
    fn is_usable_model_rejects_non_text_output() {
        let mut m = base_catalog_model();
        m.architecture = Some(Architecture {
            input_modalities: Some(vec!["text".into()]),
            output_modalities: Some(vec!["image".into()]),
        });
        assert!(!is_usable_model(&m));
    }

    #[test]
    fn is_usable_model_rejects_zero_context() {
        let mut m = base_catalog_model();
        m.context_length = Some(0);
        assert!(!is_usable_model(&m));
    }

    // ── ModelDescriptor mapping tests ───────────────────────────────

    #[test]
    fn catalog_model_maps_context_windows() {
        let m = base_catalog_model();
        let desc = catalog_model_to_descriptor(&m);
        assert_eq!(desc.context_window_in, 128_000);
        assert_eq!(desc.context_window_out, 4096);
    }

    #[test]
    fn catalog_model_maps_pricing() {
        let m = base_catalog_model();
        let desc = catalog_model_to_descriptor(&m);
        assert_eq!(desc.cost_in, 150);
        assert_eq!(desc.cost_out, 600);
    }

    #[test]
    fn catalog_model_builds_capability_driven_settings() {
        let m = base_catalog_model();
        let desc = catalog_model_to_descriptor(&m);
        let keys: Vec<&str> = desc.settings.iter().map(|s| s.key.as_str()).collect();
        assert!(keys.contains(&"max_output_tokens"));
        assert!(keys.contains(&"temperature"));
        assert!(!keys.contains(&"top_p")); // Not in supported_parameters.
    }

    #[test]
    fn catalog_model_fallback_context_out() {
        let mut m = base_catalog_model();
        m.top_provider = None;
        let desc = catalog_model_to_descriptor(&m);
        assert_eq!(desc.context_window_out, 4096); // Conservative fallback.
    }

    // ── Default model selection tests ───────────────────────────────

    fn desc(id: &str) -> ModelDescriptor {
        ModelDescriptor {
            id: id.into(),
            name: id.into(),
            description: String::new(),
            is_default: false,
            settings: vec![],
            context_window_in: 128_000,
            context_window_out: 4096,
            knowledge_cutoff: "unknown".into(),
            cost_in: 0,
            cost_out: 0,
        }
    }

    #[test]
    fn pick_default_prefers_gpt4o_mini() {
        let descriptors = vec![
            desc("anthropic/claude-sonnet-4"),
            desc("openai/gpt-4o-mini"),
            desc("google/gemini-2.5-flash"),
        ];
        assert_eq!(pick_default_index(&descriptors), Some(1));
    }

    #[test]
    fn pick_default_falls_back_to_first() {
        let descriptors = vec![desc("some/unknown-model"), desc("another/unknown-model")];
        assert_eq!(pick_default_index(&descriptors), Some(0));
    }

    #[test]
    fn pick_default_empty_returns_none() {
        let descriptors: Vec<ModelDescriptor> = vec![];
        assert_eq!(pick_default_index(&descriptors), None);
    }

    // ── Request body tests ──────────────────────────────────────────

    #[test]
    fn build_request_body_includes_model_and_provider_require() {
        let messages = vec![Message {
            role: "user".into(),
            parts: vec![MessagePart::Text("Hello".into())],
        }];
        let body = build_request_body("openai/gpt-4o-mini", &messages, &[], &[], false);
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json["model"], "openai/gpt-4o-mini");
        assert_eq!(json["stream"], false);
        assert_eq!(json["provider"]["require_parameters"], true);
    }

    #[test]
    fn build_request_body_maps_settings() {
        let messages = vec![Message {
            role: "user".into(),
            parts: vec![MessagePart::Text("Hi".into())],
        }];
        let settings = vec![
            ConfigSetting {
                key: "temperature".into(),
                value: SettingValue::Number(0.7),
            },
            ConfigSetting {
                key: "max_output_tokens".into(),
                value: SettingValue::Integer(1024),
            },
        ];
        let body = build_request_body("test/model", &messages, &settings, &[], false);
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json["temperature"], 0.7);
        assert_eq!(json["max_tokens"], 1024);
    }

    #[test]
    fn message_to_openai_tool_result() {
        let msg = Message {
            role: "tool".into(),
            parts: vec![MessagePart::ToolResult(ur::extension::types::ToolResult {
                tool_call_id: "call-1".into(),
                tool_name: "get_weather".into(),
                content: "{\"temp\":72}".into(),
            })],
        };
        let json = message_to_openai(&msg);
        assert_eq!(json["role"], "tool");
        assert_eq!(json["tool_call_id"], "call-1");
        assert_eq!(json["content"], "{\"temp\":72}");
    }

    #[test]
    fn message_to_openai_assistant_tool_calls() {
        let msg = Message {
            role: "assistant".into(),
            parts: vec![MessagePart::ToolCall(ur::extension::types::ToolCall {
                id: "call-1".into(),
                name: "get_weather".into(),
                arguments_json: "{\"city\":\"Paris\"}".into(),
                provider_metadata_json: String::new(),
            })],
        };
        let json = message_to_openai(&msg);
        assert_eq!(json["role"], "assistant");
        assert_eq!(json["tool_calls"][0]["id"], "call-1");
        assert_eq!(json["tool_calls"][0]["function"]["name"], "get_weather");
    }

    // ── Response parsing tests ──────────────────────────────────────

    #[test]
    fn parse_chat_response_text_only() {
        let body = serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "Hello!"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5
            }
        })
        .to_string();
        let completion = parse_chat_response(&body).unwrap();
        assert_eq!(extract_text(&completion.message), "Hello!");
        assert_eq!(completion.usage.unwrap().input_tokens, 10);
    }

    #[test]
    fn parse_chat_response_with_tool_calls() {
        let body = serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call-1",
                        "type": "function",
                        "function": {
                            "name": "get_weather",
                            "arguments": "{\"city\":\"Paris\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        })
        .to_string();
        let completion = parse_chat_response(&body).unwrap();
        let tc = match &completion.message.parts[0] {
            MessagePart::ToolCall(tc) => tc,
            _ => panic!("expected tool call"),
        };
        assert_eq!(tc.id, "call-1");
        assert_eq!(tc.name, "get_weather");
    }

    #[test]
    fn parse_chat_response_api_error() {
        let body = serde_json::json!({
            "error": {
                "message": "Rate limit exceeded"
            }
        })
        .to_string();
        let err = parse_chat_response(&body).unwrap_err();
        assert!(err.contains("Rate limit"));
    }

    // ── SSE parsing tests ───────────────────────────────────────────

    fn text_chunk_json(text: &str) -> String {
        serde_json::json!({
            "choices": [{
                "delta": {"content": text},
                "finish_reason": null
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
            pending_tool_calls: Vec::new(),
        }
    }

    #[test]
    fn sse_ignores_comment_lines() {
        let json = text_chunk_json("Hello");
        let mut state = stream_state(&format!(": OPENROUTER PROCESSING\ndata: {json}\n\n"));
        let chunk = try_parse_sse_event(&mut state).expect("SSE chunk");
        assert!(matches!(
            &chunk.delta_parts[0],
            MessagePart::Text(text) if text == "Hello"
        ));
    }

    #[test]
    fn sse_accumulates_tool_call_deltas() {
        let first = serde_json::json!({
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "id": "call-1",
                        "function": {"name": "get_weather", "arguments": "{\"ci"}
                    }]
                },
                "finish_reason": null
            }]
        })
        .to_string();

        let second = serde_json::json!({
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "function": {"arguments": "ty\":\"Paris\"}"}
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        })
        .to_string();

        let mut state = stream_state(&format!("data: {first}\n\ndata: {second}\n\n"));

        let first_chunk = try_parse_sse_event(&mut state).expect("first chunk");
        // First chunk should have no emitted parts (accumulating).
        assert!(first_chunk.delta_parts.is_empty());

        let second_chunk = try_parse_sse_event(&mut state).expect("second chunk");
        // Second chunk should emit the completed tool call.
        assert_eq!(second_chunk.delta_parts.len(), 1);
        let tc = match &second_chunk.delta_parts[0] {
            MessagePart::ToolCall(tc) => tc,
            _ => panic!("expected tool call"),
        };
        assert_eq!(tc.id, "call-1");
        assert_eq!(tc.name, "get_weather");
        assert_eq!(tc.arguments_json, "{\"city\":\"Paris\"}");
    }

    #[test]
    fn sse_handles_done_marker() {
        let mut pending = Vec::new();
        let result = parse_sse_event("data: [DONE]", &mut pending).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn sse_handles_mid_stream_error() {
        let error_event = serde_json::json!({
            "error": {
                "message": "Model overloaded"
            }
        })
        .to_string();
        let mut pending = Vec::new();
        let result = parse_sse_event(&format!("data: {error_event}"), &mut pending);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Model overloaded"));
    }

    #[test]
    fn sse_text_chunks_stream_correctly() {
        let first = text_chunk_json("Hello");
        let second = text_chunk_json(" world");
        let mut state = stream_state(&format!("data: {first}\n\ndata: {second}\n\n"));

        let c1 = try_parse_sse_event(&mut state).expect("first");
        let c2 = try_parse_sse_event(&mut state).expect("second");

        assert!(matches!(&c1.delta_parts[0], MessagePart::Text(t) if t == "Hello"));
        assert!(matches!(&c2.delta_parts[0], MessagePart::Text(t) if t == " world"));
    }

    #[test]
    fn sse_handles_crlf() {
        let json = text_chunk_json("Hi");
        let mut state = stream_state(&format!("data: {json}\r\n\r\n"));
        let chunk = try_parse_sse_event(&mut state).expect("CRLF chunk");
        assert!(matches!(&chunk.delta_parts[0], MessagePart::Text(t) if t == "Hi"));
    }
}
