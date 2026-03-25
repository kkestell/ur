//! Native `OpenRouter` LLM provider.
//!
//! Ports the WASM `llm-openrouter` extension to a native Rust module using
//! `reqwest` for HTTP and `futures_util::StreamExt` for SSE streaming.

use anyhow::{Context as _, bail};
use futures_util::StreamExt;
use tokio::sync::RwLock;

use crate::types::{
    Completion, CompletionChunk, ConfigSetting, Message, MessagePart, ModelDescriptor,
    SettingBoolean, SettingDescriptor, SettingInteger, SettingNumber, SettingSchema, SettingString,
    SettingValue, TextPart, ToolCall, ToolChoice, ToolDescriptor, Usage,
};

// ── Provider struct ─────────────────────────────────────────────────

#[expect(
    missing_debug_implementations,
    reason = "Contains async RwLocks that are not Debug-friendly"
)]
pub struct OpenRouterProvider {
    api_key: String,
    client: reqwest::Client,
    /// Cached model catalog fetched from `OpenRouter`.
    cached_catalog: RwLock<Option<Vec<CatalogModel>>>,
    cached_settings: RwLock<Option<Vec<SettingDescriptor>>>,
}

impl OpenRouterProvider {
    #[must_use]
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: reqwest::Client::new(),
            cached_catalog: RwLock::new(None),
            cached_settings: RwLock::new(None),
        }
    }

    pub fn provider_id(&self) -> &'static str {
        "openrouter"
    }

    pub async fn list_models(&self) -> Vec<ModelDescriptor> {
        // Return cached descriptors if available.
        {
            let cache = self.cached_catalog.read().await;
            if let Some(catalog) = cache.as_ref() {
                let mut descriptors: Vec<ModelDescriptor> =
                    catalog.iter().map(catalog_model_to_descriptor).collect();
                let default_idx = pick_default_index(&descriptors);
                if let Some(idx) = default_idx {
                    descriptors[idx].is_default = true;
                }
                return descriptors;
            }
        }

        let catalog = match self.fetch_catalog().await {
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

        // Cache the catalog and derived settings.
        let mut settings = vec![SettingDescriptor {
            key: "api_key".into(),
            name: "API Key".into(),
            description: "OpenRouter API key".into(),
            schema: SettingSchema::String(SettingString {
                default_val: String::new(),
            }),
            secret: true,
            readonly: false,
        }];
        for m in &catalog {
            settings.extend(catalog_model_settings(m));
        }

        *self.cached_settings.write().await = Some(settings);
        *self.cached_catalog.write().await = Some(catalog);

        descriptors
    }

    pub async fn list_settings(&self) -> Vec<SettingDescriptor> {
        // Ensure catalog is fetched (populates cached_settings as a side effect).
        {
            let cache = self.cached_settings.read().await;
            if let Some(settings) = cache.as_ref() {
                return settings.clone();
            }
        }

        // Force a catalog fetch to populate settings.
        let _ = self.list_models().await;

        let cache = self.cached_settings.read().await;
        cache.clone().unwrap_or_else(|| {
            vec![SettingDescriptor {
                key: "api_key".into(),
                name: "API Key".into(),
                description: "OpenRouter API key".into(),
                schema: SettingSchema::String(SettingString {
                    default_val: String::new(),
                }),
                secret: true,
                readonly: false,
            }]
        })
    }

    async fn complete_async(
        &self,
        messages: &[Message],
        model_id: &str,
        settings: &[ConfigSetting],
        tools: &[ToolDescriptor],
        tool_choice: Option<&ToolChoice>,
        on_chunk: &mut dyn FnMut(CompletionChunk),
    ) -> anyhow::Result<Completion> {
        let body = build_request_body(model_id, messages, settings, tools, tool_choice);

        let response = self
            .client
            .post("https://openrouter.ai/api/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .body(body)
            .send()
            .await
            .context("OpenRouter HTTP request failed")?;

        let status = response.status();
        if !status.is_success() {
            let err_text = response
                .text()
                .await
                .unwrap_or_else(|_| String::from("<unreadable>"));
            bail!("OpenRouter API error: HTTP {status}: {err_text}");
        }

        // Stream the SSE response.
        let mut stream = response.bytes_stream();
        let mut sse_state = SseState {
            buffer: String::new(),
            pos: 0,
            pending_tool_calls: Vec::new(),
        };

        let mut accumulated_parts: Vec<MessagePart> = Vec::new();
        let mut final_usage: Option<Usage> = None;

        while let Some(chunk_result) = stream.next().await {
            let bytes = chunk_result.context("error reading SSE stream")?;
            let text = String::from_utf8_lossy(&bytes);
            sse_state.buffer.push_str(&text);

            while let Some(chunk) = try_parse_sse_event(&mut sse_state) {
                // Track usage from the last chunk that carries it.
                if chunk.usage.is_some() {
                    final_usage.clone_from(&chunk.usage);
                }

                // Accumulate parts for the final Completion message.
                for part in &chunk.delta_parts {
                    accumulate_part(&mut accumulated_parts, part);
                }

                on_chunk(chunk);
            }
        }

        // Drain any remaining buffered events after stream ends.
        while let Some(chunk) = try_parse_sse_event(&mut sse_state) {
            if chunk.usage.is_some() {
                final_usage.clone_from(&chunk.usage);
            }
            for part in &chunk.delta_parts {
                accumulate_part(&mut accumulated_parts, part);
            }
            on_chunk(chunk);
        }

        Ok(Completion {
            message: Message {
                role: "assistant".into(),
                parts: accumulated_parts,
            },
            usage: final_usage,
        })
    }

    /// Fetches the model catalog from `OpenRouter`'s API.
    async fn fetch_catalog(&self) -> anyhow::Result<Vec<CatalogModel>> {
        let response = self
            .client
            .get("https://openrouter.ai/api/v1/models?supported_parameters=tools&output_modalities=text")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .send()
            .await
            .context("failed to fetch OpenRouter model catalog")?;

        let status = response.status();
        if !status.is_success() {
            let err_text = response
                .text()
                .await
                .unwrap_or_else(|_| String::from("<unreadable>"));
            bail!("OpenRouter API error: HTTP {status}: {err_text}");
        }

        let body = response
            .text()
            .await
            .context("failed to read catalog response body")?;
        let catalog_response: CatalogResponse =
            serde_json::from_str(&body).context("failed to parse catalog JSON")?;

        let mut filtered: Vec<CatalogModel> = catalog_response
            .data
            .into_iter()
            .filter(is_usable_model)
            .collect();

        filtered.sort_by(|a, b| a.id.cmp(&b.id));

        Ok(filtered)
    }
}

impl super::LlmProvider for OpenRouterProvider {
    fn provider_id(&self) -> &'static str {
        "openrouter"
    }

    fn list_models(&self) -> Vec<ModelDescriptor> {
        let handle = tokio::runtime::Handle::current();
        handle.block_on(self.list_models())
    }

    fn list_settings(&self) -> Vec<SettingDescriptor> {
        let handle = tokio::runtime::Handle::current();
        handle.block_on(self.list_settings())
    }

    fn complete(
        &self,
        messages: &[Message],
        model_id: &str,
        settings: &[ConfigSetting],
        tools: &[ToolDescriptor],
        tool_choice: Option<&ToolChoice>,
        on_chunk: &mut dyn FnMut(CompletionChunk),
    ) -> anyhow::Result<Completion> {
        let handle = tokio::runtime::Handle::current();
        handle.block_on(self.complete_async(
            messages,
            model_id,
            settings,
            tools,
            tool_choice,
            on_chunk,
        ))
    }
}

// ── SSE streaming state ─────────────────────────────────────────────

struct SseState {
    buffer: String,
    pos: usize,
    pending_tool_calls: Vec<PendingToolCall>,
}

#[derive(Clone)]
struct PendingToolCall {
    index: u32,
    id: String,
    name: String,
    arguments: String,
}

// ── SSE parsing ─────────────────────────────────────────────────────

fn try_parse_sse_event(state: &mut SseState) -> Option<CompletionChunk> {
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

fn trim_consumed_buffer(state: &mut SseState) {
    if state.pos == 0 {
        return;
    }

    state.buffer.drain(..state.pos);
    state.pos = 0;
}

fn parse_sse_event(
    event: &str,
    pending_tool_calls: &mut Vec<PendingToolCall>,
) -> anyhow::Result<Option<CompletionChunk>> {
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
) -> anyhow::Result<CompletionChunk> {
    let response: serde_json::Value = serde_json::from_str(json_str).context("invalid SSE JSON")?;

    // Check for mid-stream error events.
    if let Some(error) = response.get("error") {
        let msg = error
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown error");
        bail!("OpenRouter streaming error: {msg}");
    }

    let choices = response
        .get("choices")
        .and_then(|c| c.as_array())
        .context("missing choices in SSE chunk")?;
    let choice = choices.first().context("empty choices in SSE chunk")?;
    let delta = choice.get("delta").context("missing delta in SSE chunk")?;

    let mut delta_parts = Vec::new();

    // Text content.
    if let Some(content) = delta.get("content").and_then(|c| c.as_str())
        && !content.is_empty()
    {
        delta_parts.push(MessagePart::Text(TextPart {
            text: content.to_string(),
        }));
    }

    // Tool call deltas -- accumulate across chunks.
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

    // Emit tool calls eagerly as soon as their arguments are valid JSON.
    pending_tool_calls.retain(|tc| {
        if !tc.arguments.is_empty()
            && serde_json::from_str::<serde_json::Value>(&tc.arguments).is_ok()
        {
            delta_parts.push(MessagePart::ToolCall(ToolCall {
                id: tc.id.clone(),
                name: tc.name.clone(),
                arguments_json: tc.arguments.clone(),
                provider_metadata_json: String::new(),
            }));
            false // remove from pending
        } else {
            true // keep accumulating
        }
    });

    let usage = response.get("usage").and_then(parse_usage);

    Ok(CompletionChunk { delta_parts, usage })
}

// ── Catalog types ───────────────────────────────────────────────────

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

// ── Catalog filtering ───────────────────────────────────────────────

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
    ModelDescriptor {
        id: m.id.clone(),
        name: m.name.clone(),
        description: m.description.clone(),
        is_default: false,
    }
}

// ── Per-model settings ──────────────────────────────────────────────

/// Builds the full dotted-key settings namespace for a catalog model.
fn catalog_model_settings(m: &CatalogModel) -> Vec<SettingDescriptor> {
    let context_in = u32::try_from(m.context_length.unwrap_or(0)).unwrap_or(u32::MAX);
    let context_out = m
        .top_provider
        .as_ref()
        .and_then(|tp| tp.max_completion_tokens)
        .map_or(4096, |v| u32::try_from(v).unwrap_or(u32::MAX));
    let (cost_in, cost_out) = convert_pricing(m);

    let id = &m.id;
    let mut settings = build_model_settings(m, context_out, id);

    // Readonly metadata.
    settings.push(SettingDescriptor {
        key: format!("{id}.context_window_in"),
        name: "Context Window (input)".into(),
        description: "Maximum input tokens".into(),
        schema: SettingSchema::Integer(SettingInteger {
            min: 0,
            max: i64::from(context_in),
            default_val: i64::from(context_in),
        }),
        secret: false,
        readonly: true,
    });
    settings.push(SettingDescriptor {
        key: format!("{id}.context_window_out"),
        name: "Context Window (output)".into(),
        description: "Maximum output tokens".into(),
        schema: SettingSchema::Integer(SettingInteger {
            min: 0,
            max: i64::from(context_out),
            default_val: i64::from(context_out),
        }),
        secret: false,
        readonly: true,
    });
    settings.push(SettingDescriptor {
        key: format!("{id}.cost_in"),
        name: "Input Cost".into(),
        description: "Millidollars per million input tokens".into(),
        schema: SettingSchema::Integer(SettingInteger {
            min: 0,
            max: i64::from(cost_in),
            default_val: i64::from(cost_in),
        }),
        secret: false,
        readonly: true,
    });
    settings.push(SettingDescriptor {
        key: format!("{id}.cost_out"),
        name: "Output Cost".into(),
        description: "Millidollars per million output tokens".into(),
        schema: SettingSchema::Integer(SettingInteger {
            min: 0,
            max: i64::from(cost_out),
            default_val: i64::from(cost_out),
        }),
        secret: false,
        readonly: true,
    });

    settings
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

fn build_model_settings(m: &CatalogModel, max_output: u32, id: &str) -> Vec<SettingDescriptor> {
    let supported = m.supported_parameters.as_deref().unwrap_or(&[]);
    let mut settings = Vec::new();

    if has_param(supported, "max_tokens") || has_param(supported, "max_completion_tokens") {
        settings.push(SettingDescriptor {
            key: format!("{id}.max_output_tokens"),
            name: "Max Output Tokens".into(),
            description: "Maximum number of tokens to generate".into(),
            schema: SettingSchema::Integer(SettingInteger {
                min: 1,
                max: i64::from(max_output),
                default_val: i64::from(max_output.min(4096)),
            }),
            secret: false,
            readonly: false,
        });
    }

    if has_param(supported, "temperature") {
        settings.push(SettingDescriptor {
            key: format!("{id}.temperature"),
            name: "Temperature".into(),
            description: "Sampling temperature (0.0 = deterministic, 2.0 = creative)".into(),
            schema: SettingSchema::Number(SettingNumber {
                min: 0.0,
                max: 2.0,
                default_val: 1.0,
            }),
            secret: false,
            readonly: false,
        });
    }

    if has_param(supported, "top_p") {
        settings.push(SettingDescriptor {
            key: format!("{id}.top_p"),
            name: "Top P".into(),
            description: "Nucleus sampling threshold".into(),
            schema: SettingSchema::Number(SettingNumber {
                min: 0.0,
                max: 1.0,
                default_val: 1.0,
            }),
            secret: false,
            readonly: false,
        });
    }

    if has_param(supported, "frequency_penalty") {
        settings.push(SettingDescriptor {
            key: format!("{id}.frequency_penalty"),
            name: "Frequency Penalty".into(),
            description: "Penalizes token frequency in output".into(),
            schema: SettingSchema::Number(SettingNumber {
                min: -2.0,
                max: 2.0,
                default_val: 0.0,
            }),
            secret: false,
            readonly: false,
        });
    }

    if has_param(supported, "presence_penalty") {
        settings.push(SettingDescriptor {
            key: format!("{id}.presence_penalty"),
            name: "Presence Penalty".into(),
            description: "Penalizes token presence in output".into(),
            schema: SettingSchema::Number(SettingNumber {
                min: -2.0,
                max: 2.0,
                default_val: 0.0,
            }),
            secret: false,
            readonly: false,
        });
    }

    if has_param(supported, "seed") {
        settings.push(SettingDescriptor {
            key: format!("{id}.seed"),
            name: "Seed".into(),
            description: "Random seed for deterministic generation".into(),
            schema: SettingSchema::Integer(SettingInteger {
                min: 0,
                max: i64::from(i32::MAX),
                default_val: 0,
            }),
            secret: false,
            readonly: false,
        });
    }

    if has_param(supported, "parallel_tool_calls") {
        settings.push(SettingDescriptor {
            key: format!("{id}.parallel_tool_calls"),
            name: "Parallel Tool Calls".into(),
            description: "Allow the model to make multiple tool calls in parallel".into(),
            schema: SettingSchema::Boolean(SettingBoolean { default_val: true }),
            secret: false,
            readonly: false,
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
    tool_choice: Option<&ToolChoice>,
) -> String {
    let mut body = serde_json::Map::new();

    body.insert("model".into(), serde_json::json!(model));
    body.insert("stream".into(), serde_json::json!(true));

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

        // Map tool_choice to OpenAI format.
        if let Some(tc) = tool_choice {
            let value = match tc {
                ToolChoice::Auto => serde_json::json!("auto"),
                ToolChoice::None => serde_json::json!("none"),
                ToolChoice::Required => serde_json::json!("required"),
                ToolChoice::Specific(name) => serde_json::json!({
                    "type": "function",
                    "function": {"name": name}
                }),
            };
            body.insert("tool_choice".into(), value);
        }
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
            MessagePart::Text(t) => Some(t.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

fn parse_usage(usage: &serde_json::Value) -> Option<Usage> {
    Some(Usage {
        prompt_tokens: u32::try_from(usage.get("prompt_tokens")?.as_u64()?).ok()?,
        completion_tokens: u32::try_from(usage.get("completion_tokens")?.as_u64()?).ok()?,
    })
}

// ── Helpers ─────────────────────────────────────────────────────────

/// Accumulates a delta part into the running list for the final message.
/// Consecutive text parts are merged into a single `TextPart`.
fn accumulate_part(parts: &mut Vec<MessagePart>, delta: &MessagePart) {
    match delta {
        MessagePart::Text(t) => {
            if let Some(MessagePart::Text(last)) = parts.last_mut() {
                last.text.push_str(&t.text);
            } else {
                parts.push(MessagePart::Text(TextPart {
                    text: t.text.clone(),
                }));
            }
        }
        MessagePart::ToolCall(tc) => {
            parts.push(MessagePart::ToolCall(tc.clone()));
        }
        MessagePart::ToolResult(tr) => {
            parts.push(MessagePart::ToolResult(tr.clone()));
        }
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ToolResult;

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

    // ── Default model selection tests ───────────────────────────────

    fn desc(id: &str) -> ModelDescriptor {
        ModelDescriptor {
            id: id.into(),
            name: id.into(),
            description: String::new(),
            is_default: false,
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
        let messages = vec![Message::text("user", "Hello")];
        let body = build_request_body("openai/gpt-4o-mini", &messages, &[], &[], None);
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json["model"], "openai/gpt-4o-mini");
        assert_eq!(json["stream"], true);
        assert_eq!(json["provider"]["require_parameters"], true);
    }

    #[test]
    fn build_request_body_maps_settings() {
        let messages = vec![Message::text("user", "Hi")];
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
        let body = build_request_body("test/model", &messages, &settings, &[], None);
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json["temperature"], 0.7);
        assert_eq!(json["max_tokens"], 1024);
    }

    #[test]
    fn message_to_openai_tool_result() {
        let msg = Message {
            role: "tool".into(),
            parts: vec![MessagePart::ToolResult(ToolResult {
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
            parts: vec![MessagePart::ToolCall(ToolCall {
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

    #[test]
    fn parse_sse_event_text_content() {
        let event = format!("data: {}\n", text_chunk_json("Hello"));
        let mut pending = Vec::new();
        let chunk = parse_sse_event(&event, &mut pending).unwrap().unwrap();
        assert_eq!(chunk.delta_parts.len(), 1);
        match &chunk.delta_parts[0] {
            MessagePart::Text(t) => assert_eq!(t.text, "Hello"),
            _ => panic!("expected text part"),
        }
    }

    #[test]
    fn parse_sse_event_done_with_pending_tool_calls() {
        let mut pending = vec![PendingToolCall {
            index: 0,
            id: "call-1".into(),
            name: "get_weather".into(),
            arguments: "{\"city\":\"Paris\"}".into(),
        }];
        let chunk = parse_sse_event("data: [DONE]\n", &mut pending)
            .unwrap()
            .unwrap();
        assert_eq!(chunk.delta_parts.len(), 1);
        match &chunk.delta_parts[0] {
            MessagePart::ToolCall(tc) => {
                assert_eq!(tc.id, "call-1");
                assert_eq!(tc.name, "get_weather");
                assert_eq!(tc.arguments_json, "{\"city\":\"Paris\"}");
            }
            _ => panic!("expected tool call part"),
        }
    }

    #[test]
    fn parse_sse_event_skips_comments() {
        let event = ": OPENROUTER PROCESSING\n";
        let mut pending = Vec::new();
        let result = parse_sse_event(event, &mut pending).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn parse_sse_event_done_no_pending() {
        let mut pending = Vec::new();
        let result = parse_sse_event("data: [DONE]\n", &mut pending).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn tool_call_delta_accumulation() {
        let mut pending = Vec::new();

        // First chunk: tool call start with partial args.
        let chunk1 = serde_json::json!({
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "id": "call-1",
                        "function": {
                            "name": "get_weather",
                            "arguments": "{\"city\":"
                        }
                    }]
                },
                "finish_reason": null
            }]
        })
        .to_string();

        let result1 = parse_sse_chunk(&chunk1, &mut pending).unwrap();
        // Arguments are not valid JSON yet, so no tool call emitted.
        assert!(result1.delta_parts.is_empty());
        assert_eq!(pending.len(), 1);

        // Second chunk: completes the arguments.
        let chunk2 = serde_json::json!({
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "function": {
                            "arguments": "\"Paris\"}"
                        }
                    }]
                },
                "finish_reason": null
            }]
        })
        .to_string();

        let result2 = parse_sse_chunk(&chunk2, &mut pending).unwrap();
        // Now arguments are valid JSON, tool call should be emitted.
        assert_eq!(result2.delta_parts.len(), 1);
        match &result2.delta_parts[0] {
            MessagePart::ToolCall(tc) => {
                assert_eq!(tc.name, "get_weather");
                assert_eq!(tc.arguments_json, "{\"city\":\"Paris\"}");
            }
            _ => panic!("expected tool call"),
        }
        assert!(pending.is_empty());
    }

    #[test]
    fn next_complete_sse_event_basic() {
        let buffer = "data: hello\n\ndata: world\n\n";
        let (consumed, event) = next_complete_sse_event(buffer).unwrap();
        assert_eq!(event, "data: hello\n");
        let (consumed2, event2) = next_complete_sse_event(&buffer[consumed..]).unwrap();
        assert_eq!(event2, "data: world\n");
        assert_eq!(consumed + consumed2, buffer.len());
    }

    #[test]
    fn next_complete_sse_event_incomplete() {
        let buffer = "data: partial";
        assert!(next_complete_sse_event(buffer).is_none());
    }

    #[test]
    fn accumulate_part_merges_text() {
        let mut parts = Vec::new();
        accumulate_part(
            &mut parts,
            &MessagePart::Text(TextPart {
                text: "Hello".into(),
            }),
        );
        accumulate_part(
            &mut parts,
            &MessagePart::Text(TextPart {
                text: " world".into(),
            }),
        );
        assert_eq!(parts.len(), 1);
        match &parts[0] {
            MessagePart::Text(t) => assert_eq!(t.text, "Hello world"),
            _ => panic!("expected text"),
        }
    }

    #[test]
    fn accumulate_part_tool_call_breaks_text_merge() {
        let mut parts = Vec::new();
        accumulate_part(
            &mut parts,
            &MessagePart::Text(TextPart {
                text: "Hello".into(),
            }),
        );
        accumulate_part(
            &mut parts,
            &MessagePart::ToolCall(ToolCall {
                id: "call-1".into(),
                name: "test".into(),
                arguments_json: "{}".into(),
                provider_metadata_json: String::new(),
            }),
        );
        accumulate_part(
            &mut parts,
            &MessagePart::Text(TextPart {
                text: " world".into(),
            }),
        );
        assert_eq!(parts.len(), 3);
    }

    #[test]
    fn extract_text_from_message() {
        let msg = Message {
            role: "assistant".into(),
            parts: vec![
                MessagePart::Text(TextPart {
                    text: "Hello ".into(),
                }),
                MessagePart::Text(TextPart {
                    text: "world".into(),
                }),
            ],
        };
        assert_eq!(extract_text(&msg), "Hello world");
    }

    #[test]
    fn parse_usage_from_json() {
        let json = serde_json::json!({
            "prompt_tokens": 100,
            "completion_tokens": 50
        });
        let usage = parse_usage(&json).unwrap();
        assert_eq!(usage.prompt_tokens, 100);
        assert_eq!(usage.completion_tokens, 50);
    }

    #[test]
    fn parse_usage_missing_field_returns_none() {
        let json = serde_json::json!({"prompt_tokens": 100});
        assert!(parse_usage(&json).is_none());
    }
}
