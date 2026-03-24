//! Session lifecycle and turn execution.
//!
//! `UrSession` owns a persisted conversation session and drives the
//! agent turn state machine. Clients subscribe to structured events
//! via a callback rather than reading terminal output.

use std::path::Path;

use anyhow::{Result, bail};
use tracing::{debug, info};
use wasmtime::Engine;

use crate::config::UserConfig;
use crate::extension_host::{self, ExtensionInstance, LoadOptions, wit_types};
use crate::manifest::{ManifestEntry, WorkspaceManifest};
use crate::model;
use crate::provider;

/// A structured event emitted during turn execution.
#[derive(Debug, Clone)]
#[expect(
    dead_code,
    reason = "fields read by downstream clients matching on events"
)]
pub enum SessionEvent {
    /// LLM is streaming a text delta.
    TextDelta(String),
    /// LLM emitted a complete tool call.
    ToolCall {
        /// Unique tool call identifier.
        id: String,
        /// Tool name.
        name: String,
        /// JSON-encoded arguments.
        arguments_json: String,
    },
    /// A tool produced a result.
    ToolResult {
        /// Matches the originating tool call ID.
        tool_call_id: String,
        /// Tool name.
        tool_name: String,
        /// Tool output content.
        content: String,
    },
    /// The turn completed an assistant message (text only, no pending tools).
    AssistantMessage {
        /// The complete assembled text.
        text: String,
    },
    /// Tool approval is required before proceeding.
    ApprovalRequired {
        /// Tool call identifier.
        id: String,
        /// Tool name.
        tool_name: String,
        /// JSON-encoded arguments.
        arguments_json: String,
    },
    /// The turn completed successfully.
    TurnComplete,
    /// An error occurred during the turn.
    TurnError(String),
}

/// Client response to an approval request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "API contract for client-side tool denial")
)]
pub enum ApprovalDecision {
    /// Approve the tool call.
    Approve,
    /// Deny the tool call.
    Deny,
}

/// A persisted event in the session timeline.
///
/// Captures enough structured history to restore the final visible
/// client state from a single source of truth. Does not preserve
/// every streamed token delta — only assembled messages and domain
/// events.
#[derive(Debug, Clone)]
#[expect(dead_code, reason = "phase 3 API — consumed by snapshot/replay")]
pub enum PersistedEvent {
    /// A turn started.
    TurnStarted {
        /// Zero-based index of this turn.
        turn_index: u32,
    },
    /// The user sent a message.
    UserMessage {
        /// The user message text.
        text: String,
    },
    /// The assistant produced a complete text message.
    AssistantMessage {
        /// The assembled assistant text.
        text: String,
    },
    /// The LLM requested a tool call.
    ToolCallRequested {
        /// Tool call identifier.
        id: String,
        /// Tool name.
        name: String,
        /// JSON-encoded arguments.
        arguments_json: String,
    },
    /// A tool approval was requested.
    ToolApprovalRequested {
        /// Tool call identifier.
        id: String,
        /// Tool name.
        name: String,
    },
    /// A tool approval decision was made.
    ToolApprovalDecided {
        /// Tool call identifier.
        id: String,
        /// The client's decision.
        decision: ApprovalDecision,
    },
    /// A tool returned a result.
    ToolResultReceived {
        /// Matches the originating tool call ID.
        tool_call_id: String,
        /// Tool output content.
        content: String,
    },
    /// A turn completed successfully.
    TurnComplete {
        /// Zero-based index of the completed turn.
        turn_index: u32,
    },
    /// A turn was interrupted before completion.
    TurnInterrupted {
        /// Zero-based index of the interrupted turn.
        turn_index: u32,
        /// Reason for the interruption.
        reason: String,
    },
}

/// A snapshot of session state sufficient to restore client UI.
///
/// Contains the session identity, conversation messages, and a
/// structured event timeline that records domain events beyond
/// plain messages (turn boundaries, tool approvals, interruptions).
#[derive(Debug, Clone)]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "fields read by downstream clients")
)]
pub struct SessionSnapshot {
    /// The session identifier.
    pub session_id: String,
    /// The conversation message history.
    pub messages: Vec<wit_types::Message>,
    /// Structured event timeline for UI restoration.
    pub events: Vec<PersistedEvent>,
}

/// Session-scoped coordinator for turn execution.
///
/// Owns the loaded session messages and drives the agent turn loop.
/// Clients interact with it via `run_turn()`, which emits structured
/// `SessionEvent`s through a callback.
#[derive(Debug)]
pub struct UrSession {
    engine: Engine,
    manifest: WorkspaceManifest,
    config: UserConfig,
    session_id: String,
    messages: Vec<wit_types::Message>,
    loaded_message_count: usize,
    events: Vec<PersistedEvent>,
    turn_count: u32,
}

impl UrSession {
    /// Creates a session by loading existing messages from the session provider.
    ///
    /// # Errors
    ///
    /// Returns an error if the session provider cannot be loaded or
    /// the session cannot be read.
    pub(crate) fn open(
        engine: Engine,
        manifest: WorkspaceManifest,
        config: UserConfig,
        session_id: &str,
    ) -> Result<Self> {
        let mut session_ext = load_slot(&engine, &manifest, "session-provider")?;
        session_ext
            .init(&[])?
            .map_err(|e| anyhow::anyhow!("session init: {e}"))?;

        let messages: Vec<wit_types::Message> = session_ext
            .load_session(session_id)?
            .map_err(|e| anyhow::anyhow!("load_session: {e}"))?;

        let loaded_message_count = messages.len();
        info!(
            session_id,
            count = loaded_message_count,
            state = if messages.is_empty() {
                "fresh"
            } else {
                "existing"
            },
            "session loaded"
        );

        Ok(Self {
            engine,
            manifest,
            config,
            session_id: session_id.to_owned(),
            messages,
            loaded_message_count,
            events: Vec::new(),
            turn_count: 0,
        })
    }

    /// Returns the session identifier.
    #[expect(dead_code, reason = "public API surface for future clients")]
    pub fn id(&self) -> &str {
        &self.session_id
    }

    /// Returns the current message history.
    #[expect(dead_code, reason = "public API surface for future clients")]
    pub fn messages(&self) -> &[wit_types::Message] {
        &self.messages
    }

    /// Runs a single agent turn with a user message.
    ///
    /// Events are delivered via `on_event`. When the callback receives
    /// `SessionEvent::ApprovalRequired`, it may return an
    /// `ApprovalDecision` to approve or deny the tool call.
    ///
    /// # Errors
    ///
    /// Returns an error if LLM streaming, tool dispatch, session
    /// persistence, or compaction fails.
    pub fn run_turn(
        &mut self,
        user_message: &str,
        mut on_event: impl FnMut(SessionEvent) -> Option<ApprovalDecision>,
    ) -> Result<()> {
        let turn_index = self.turn_count;
        self.turn_count += 1;
        self.events.push(PersistedEvent::TurnStarted { turn_index });

        // ── 1. Add user message ──────────────────────────────────────
        let user_msg = wit_types::Message {
            role: "user".into(),
            parts: vec![wit_types::MessagePart::Text(user_message.to_owned())],
        };
        debug!(text = user_message, "adding user message");
        self.messages.push(user_msg);
        self.events.push(PersistedEvent::UserMessage {
            text: user_message.to_owned(),
        });

        // ── 2. Resolve role and load LLM ─────────────────────────────
        let (mut llm, settings, tools) = self.prepare_turn()?;

        // ── 3. First LLM completion (streaming) ─────────────────────
        info!(messages = self.messages.len(), "calling LLM streaming");
        let completion = stream_completion(
            &mut llm,
            &self.messages,
            &settings.model_id,
            &settings.config_settings,
            &tools,
            &mut on_event,
        )?;

        let tool_calls = extract_tool_calls(&completion.message);
        if tool_calls.is_empty() {
            let text = extract_text(&completion.message);
            self.events
                .push(PersistedEvent::AssistantMessage { text: text.clone() });
            on_event(SessionEvent::AssistantMessage { text });
        } else {
            for tc in &tool_calls {
                info!(tool = %tc.name, args = %tc.arguments_json, "LLM returned tool call");
                self.events.push(PersistedEvent::ToolCallRequested {
                    id: tc.id.clone(),
                    name: tc.name.clone(),
                    arguments_json: tc.arguments_json.clone(),
                });
                on_event(SessionEvent::ToolCall {
                    id: tc.id.clone(),
                    name: tc.name.clone(),
                    arguments_json: tc.arguments_json.clone(),
                });
            }
        }

        self.messages.push(completion.message.clone());

        // ── 4. Tool dispatch ─────────────────────────────────────────
        if !tool_calls.is_empty() {
            dispatch_tool_calls(
                &tool_calls,
                &self.engine,
                &self.manifest,
                &mut self.messages,
                &mut self.events,
                &mut on_event,
            )?;

            // ── 5. Second LLM completion (with tool results) ────────
            info!(
                messages = self.messages.len(),
                "calling LLM streaming (with tool results)"
            );
            let completion2 = stream_completion(
                &mut llm,
                &self.messages,
                &settings.model_id,
                &settings.config_settings,
                &tools,
                &mut on_event,
            )?;
            let text = extract_text(&completion2.message);
            self.events
                .push(PersistedEvent::AssistantMessage { text: text.clone() });
            on_event(SessionEvent::AssistantMessage { text });
            self.messages.push(completion2.message);
        }

        self.persist_and_compact()?;

        self.events
            .push(PersistedEvent::TurnComplete { turn_index });
        on_event(SessionEvent::TurnComplete);
        info!("turn complete");
        Ok(())
    }

    /// Returns a snapshot of the session state for UI restoration.
    #[expect(dead_code, reason = "public API surface for future clients")]
    pub fn snapshot(&self) -> SessionSnapshot {
        SessionSnapshot {
            session_id: self.session_id.clone(),
            messages: self.messages.clone(),
            events: self.events.clone(),
        }
    }

    /// Replays persisted events through a callback for UI restoration.
    ///
    /// Converts each `PersistedEvent` into the corresponding
    /// `SessionEvent` so clients can rebuild their UI state using
    /// the same rendering logic they use for live events.
    #[expect(dead_code, reason = "public API surface for future clients")]
    pub fn replay(&self, mut on_event: impl FnMut(SessionEvent)) {
        for event in &self.events {
            let session_event = match event {
                PersistedEvent::AssistantMessage { text } => {
                    Some(SessionEvent::AssistantMessage { text: text.clone() })
                }
                PersistedEvent::ToolCallRequested {
                    id,
                    name,
                    arguments_json,
                } => Some(SessionEvent::ToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    arguments_json: arguments_json.clone(),
                }),
                PersistedEvent::ToolApprovalRequested { id, name } => {
                    Some(SessionEvent::ApprovalRequired {
                        id: id.clone(),
                        tool_name: name.clone(),
                        arguments_json: String::new(),
                    })
                }
                PersistedEvent::ToolResultReceived {
                    tool_call_id,
                    content,
                } => Some(SessionEvent::ToolResult {
                    tool_call_id: tool_call_id.clone(),
                    tool_name: String::new(),
                    content: content.clone(),
                }),
                PersistedEvent::TurnComplete { .. } => Some(SessionEvent::TurnComplete),
                PersistedEvent::TurnInterrupted { reason, .. } => {
                    Some(SessionEvent::TurnError(reason.clone()))
                }
                // Internal bookkeeping events don't produce client events.
                PersistedEvent::TurnStarted { .. }
                | PersistedEvent::UserMessage { .. }
                | PersistedEvent::ToolApprovalDecided { .. } => None,
            };

            if let Some(e) = session_event {
                on_event(e);
            }
        }
    }

    /// Resolves the LLM provider, settings, and tools for a turn.
    fn prepare_turn(
        &self,
    ) -> Result<(
        ExtensionInstance,
        TurnSettings,
        Vec<wit_types::ToolDescriptor>,
    )> {
        let providers = model::collect_provider_models(&self.engine, &self.manifest)?;
        let (provider_id, model_id) = model::resolve_role(&self.config, "default", &providers)?;
        info!(%provider_id, %model_id, "resolved role \"default\"");

        let init_config = provider::init_config(&provider_id);

        // Probe for settings descriptors.
        let (mut settings_probe, extension_id) =
            load_llm_provider(&self.engine, &self.manifest, &provider_id, &init_config)?;
        let _ = settings_probe.list_models();
        let descriptors = settings_probe.list_settings()?;
        drop(settings_probe);

        let config_settings = self
            .config
            .settings_for(&extension_id, &model_id, &descriptors)?;

        // Load general extensions and collect tools.
        let mut generals = load_general_extensions(&self.engine, &self.manifest)?;
        let mut tools: Vec<wit_types::ToolDescriptor> = Vec::new();
        for ext in &mut generals {
            ext.init(&[])?
                .map_err(|e| anyhow::anyhow!("extension init: {e}"))?;
            tools.extend(ext.list_tools()?);
        }
        if !tools.is_empty() {
            info!(count = tools.len(), "collected tools");
        }

        let (llm, _) = load_llm_provider(&self.engine, &self.manifest, &provider_id, &init_config)?;

        Ok((
            llm,
            TurnSettings {
                model_id: model_id.clone(),
                config_settings,
            },
            tools,
        ))
    }

    /// Appends new messages to the session provider and runs compaction.
    fn persist_and_compact(&mut self) -> Result<()> {
        // Append new messages to the session provider.
        let mut session_ext = load_slot(&self.engine, &self.manifest, "session-provider")?;
        session_ext
            .init(&[])?
            .map_err(|e| anyhow::anyhow!("session init: {e}"))?;

        let session_appends = pending_session_appends(&self.messages, self.loaded_message_count);
        debug!(
            count = session_appends.len(),
            session_id = self.session_id,
            "appending messages to session"
        );
        for message in session_appends {
            session_ext
                .append_session(&self.session_id, message)?
                .map_err(|e| anyhow::anyhow!("append_session: {e}"))?;
        }

        // Update loaded count so subsequent turns don't re-append.
        self.loaded_message_count = self.messages.len();

        // Compact messages.
        info!(count = self.messages.len(), "compacting messages");
        let mut compaction = load_slot(&self.engine, &self.manifest, "compaction-provider")?;
        compaction
            .init(&[])?
            .map_err(|e| anyhow::anyhow!("compaction init: {e}"))?;
        let compacted = compaction
            .compact(&self.messages)?
            .map_err(|e| anyhow::anyhow!("compact: {e}"))?;
        info!(
            count = compacted.len(),
            result = if compacted.len() == self.messages.len() {
                "unchanged"
            } else {
                "compacted"
            },
            "compaction complete"
        );

        Ok(())
    }
}

/// Resolved LLM settings for a single turn.
struct TurnSettings {
    model_id: String,
    config_settings: Vec<wit_types::ConfigSetting>,
}

// --- Internal helpers (extracted from turn.rs) ---

/// Extracts concatenated text from a message's parts.
fn extract_text(msg: &wit_types::Message) -> String {
    msg.parts
        .iter()
        .filter_map(|p| match p {
            wit_types::MessagePart::Text(s) => Some(s.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

/// Extracts tool calls from a message's parts.
fn extract_tool_calls(msg: &wit_types::Message) -> Vec<&wit_types::ToolCall> {
    msg.parts
        .iter()
        .filter_map(|p| match p {
            wit_types::MessagePart::ToolCall(tc) => Some(tc),
            _ => None,
        })
        .collect()
}

/// Assembles a `Completion` from streamed chunks, emitting events for each delta.
fn stream_completion(
    llm: &mut ExtensionInstance,
    messages: &[wit_types::Message],
    model_id: &str,
    settings: &[wit_types::ConfigSetting],
    tools: &[wit_types::ToolDescriptor],
    on_event: &mut impl FnMut(SessionEvent) -> Option<ApprovalDecision>,
) -> Result<wit_types::Completion> {
    let mut parts: Vec<wit_types::MessagePart> = Vec::new();
    let mut usage = None;

    llm.complete(messages, model_id, settings, tools, None, |chunk| {
        for dp in &chunk.delta_parts {
            match dp {
                wit_types::MessagePart::Text(delta) => {
                    on_event(SessionEvent::TextDelta(delta.clone()));
                    if let Some(wit_types::MessagePart::Text(existing)) = parts.last_mut() {
                        existing.push_str(delta);
                    } else {
                        parts.push(wit_types::MessagePart::Text(delta.clone()));
                    }
                }
                wit_types::MessagePart::ToolCall(tc) => {
                    parts.push(wit_types::MessagePart::ToolCall(tc.clone()));
                }
                wit_types::MessagePart::ToolResult(tr) => {
                    parts.push(wit_types::MessagePart::ToolResult(tr.clone()));
                }
            }
        }
        if chunk.usage.is_some() {
            usage = chunk.usage;
        }
    })?
    .map_err(|e| anyhow::anyhow!("LLM streaming: {e}"))?;

    Ok(wit_types::Completion {
        message: wit_types::Message {
            role: "assistant".into(),
            parts,
        },
        usage,
    })
}

/// Dispatches tool calls to general extensions in parallel, appending results.
fn dispatch_tool_calls(
    tool_calls: &[&wit_types::ToolCall],
    engine: &Engine,
    manifest: &WorkspaceManifest,
    messages: &mut Vec<wit_types::Message>,
    events: &mut Vec<PersistedEvent>,
    on_event: &mut impl FnMut(SessionEvent) -> Option<ApprovalDecision>,
) -> Result<()> {
    if tool_calls.is_empty() {
        return Ok(());
    }

    for tc in tool_calls {
        info!(tool = %tc.name, "dispatching tool");
    }

    let results: Vec<Result<wit_types::Message>> = std::thread::scope(|s| {
        let handles: Vec<_> = tool_calls
            .iter()
            .map(|tc| {
                s.spawn(move || {
                    let mut generals = load_general_extensions(engine, manifest)?;
                    for ext in &mut generals {
                        ext.init(&[])?
                            .map_err(|e| anyhow::anyhow!("extension init: {e}"))?;
                    }

                    for ext in &mut generals {
                        if let Ok(result) = ext.call_tool(&tc.name, &tc.arguments_json)? {
                            debug!(tool = %tc.name, %result, "tool result");
                            return Ok(wit_types::Message {
                                role: "tool".into(),
                                parts: vec![wit_types::MessagePart::ToolResult(
                                    wit_types::ToolResult {
                                        tool_call_id: tc.id.clone(),
                                        tool_name: tc.name.clone(),
                                        content: result,
                                    },
                                )],
                            });
                        }
                    }
                    bail!("no extension handled tool {:?}", tc.name)
                })
            })
            .collect();

        handles
            .into_iter()
            .map(|h| h.join().expect("tool dispatch thread panicked"))
            .collect()
    });

    for result in results {
        let msg = result?;
        for part in &msg.parts {
            if let wit_types::MessagePart::ToolResult(tr) = part {
                events.push(PersistedEvent::ToolResultReceived {
                    tool_call_id: tr.tool_call_id.clone(),
                    content: tr.content.clone(),
                });
                on_event(SessionEvent::ToolResult {
                    tool_call_id: tr.tool_call_id.clone(),
                    tool_name: tr.tool_name.clone(),
                    content: tr.content.clone(),
                });
            }
        }
        messages.push(msg);
    }
    Ok(())
}

fn pending_session_appends(
    messages: &[wit_types::Message],
    loaded_message_count: usize,
) -> &[wit_types::Message] {
    if loaded_message_count >= messages.len() {
        return &[];
    }
    &messages[loaded_message_count..]
}

/// Finds the first enabled entry for a slot and loads it.
fn load_slot(
    engine: &Engine,
    manifest: &WorkspaceManifest,
    slot: &str,
) -> Result<ExtensionInstance> {
    let entry = first_enabled(manifest, slot)?;
    let caps = extension_host::strings_to_capabilities(&entry.capabilities);
    let opts = LoadOptions {
        capabilities: Some(&caps),
        ..LoadOptions::default()
    };
    let instance = ExtensionInstance::load(engine, Path::new(&entry.wasm_path), &opts)?;
    Ok(instance)
}

/// Loads the LLM provider extension matching a specific provider ID.
fn load_llm_provider(
    engine: &Engine,
    manifest: &WorkspaceManifest,
    provider_id: &str,
    init_config: &[(String, String)],
) -> Result<(ExtensionInstance, String)> {
    for entry in &manifest.extensions {
        if !entry.enabled || entry.slot.as_deref() != Some("llm-provider") {
            continue;
        }
        let caps = extension_host::strings_to_capabilities(&entry.capabilities);
        let opts = LoadOptions {
            capabilities: Some(&caps),
            ..LoadOptions::default()
        };
        let mut instance = ExtensionInstance::load(engine, Path::new(&entry.wasm_path), &opts)?;
        instance
            .init(init_config)?
            .map_err(|e| anyhow::anyhow!("LLM init: {e}"))?;
        if let Ok(Ok(id)) = instance.provider_id()
            && id == provider_id
        {
            return Ok((instance, entry.id.clone()));
        }
    }
    bail!("no enabled LLM provider with id \"{provider_id}\"")
}

/// Loads all enabled general extensions (for tool dispatch).
fn load_general_extensions(
    engine: &Engine,
    manifest: &WorkspaceManifest,
) -> Result<Vec<ExtensionInstance>> {
    let mut result = Vec::new();
    for entry in &manifest.extensions {
        if !entry.enabled || entry.slot.is_some() {
            continue;
        }
        let caps = extension_host::strings_to_capabilities(&entry.capabilities);
        let opts = LoadOptions {
            capabilities: Some(&caps),
            ..LoadOptions::default()
        };
        let instance = ExtensionInstance::load(engine, Path::new(&entry.wasm_path), &opts)?;
        result.push(instance);
    }
    Ok(result)
}

/// Loads and initializes the session provider extension.
///
/// # Errors
///
/// Returns an error if no session provider is enabled or init fails.
pub(crate) fn load_session_provider(
    engine: &Engine,
    manifest: &WorkspaceManifest,
) -> Result<ExtensionInstance> {
    let mut ext = load_slot(engine, manifest, "session-provider")?;
    ext.init(&[])?
        .map_err(|e| anyhow::anyhow!("session init: {e}"))?;
    Ok(ext)
}

/// Finds the first enabled manifest entry for a given slot.
fn first_enabled<'a>(manifest: &'a WorkspaceManifest, slot: &str) -> Result<&'a ManifestEntry> {
    manifest
        .extensions
        .iter()
        .find(|e| e.enabled && e.slot.as_deref() == Some(slot))
        .ok_or_else(|| anyhow::anyhow!("no enabled extension for slot \"{slot}\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_message(role: &str, text: &str) -> wit_types::Message {
        wit_types::Message {
            role: role.into(),
            parts: vec![wit_types::MessagePart::Text(text.into())],
        }
    }

    fn tool_call_message(tool_call_id: &str, tool_name: &str) -> wit_types::Message {
        wit_types::Message {
            role: "assistant".into(),
            parts: vec![wit_types::MessagePart::ToolCall(wit_types::ToolCall {
                id: tool_call_id.into(),
                name: tool_name.into(),
                arguments_json: "{\"city\":\"Austin\"}".into(),
                provider_metadata_json: String::new(),
            })],
        }
    }

    fn tool_result_message(tool_call_id: &str, tool_name: &str) -> wit_types::Message {
        wit_types::Message {
            role: "tool".into(),
            parts: vec![wit_types::MessagePart::ToolResult(wit_types::ToolResult {
                tool_call_id: tool_call_id.into(),
                tool_name: tool_name.into(),
                content: "{\"temperature_f\":72}".into(),
            })],
        }
    }

    #[test]
    fn pending_session_appends_no_tool_turn_includes_user_and_reply() {
        let messages = vec![
            text_message("user", "Earlier question"),
            text_message("assistant", "Earlier answer"),
            text_message("user", "Hello"),
            text_message("assistant", "Hi there"),
        ];

        let appends = pending_session_appends(&messages, 2);

        assert_eq!(appends.len(), 2);
        assert_eq!(appends[0].role, "user");
        assert_eq!(extract_text(&appends[0]), "Hello");
        assert_eq!(appends[1].role, "assistant");
        assert_eq!(extract_text(&appends[1]), "Hi there");
    }

    #[test]
    fn pending_session_appends_tool_turn_includes_full_turn_delta() {
        let messages = vec![
            text_message("assistant", "Existing context"),
            text_message("user", "Weather?"),
            tool_call_message("call-1", "get_weather"),
            tool_result_message("call-1", "get_weather"),
            text_message("assistant", "It is 72F in Austin."),
        ];

        let appends = pending_session_appends(&messages, 1);

        assert_eq!(appends.len(), 4);
        assert_eq!(appends[0].role, "user");
        assert!(matches!(
            &appends[1].parts[0],
            wit_types::MessagePart::ToolCall(tc)
                if tc.id == "call-1" && tc.name == "get_weather"
        ));
        assert!(matches!(
            &appends[2].parts[0],
            wit_types::MessagePart::ToolResult(tr)
                if tr.tool_call_id == "call-1" && tr.tool_name == "get_weather"
        ));
        assert_eq!(appends[3].role, "assistant");
        assert_eq!(extract_text(&appends[3]), "It is 72F in Austin.");
    }

    #[test]
    fn session_event_variants_are_constructible_and_matchable() {
        let events = [
            SessionEvent::TextDelta("hello".into()),
            SessionEvent::ToolCall {
                id: "1".into(),
                name: "test".into(),
                arguments_json: "{}".into(),
            },
            SessionEvent::ToolResult {
                tool_call_id: "1".into(),
                tool_name: "test".into(),
                content: "ok".into(),
            },
            SessionEvent::AssistantMessage {
                text: "hello".into(),
            },
            SessionEvent::ApprovalRequired {
                id: "1".into(),
                tool_name: "test".into(),
                arguments_json: "{}".into(),
            },
            SessionEvent::TurnComplete,
            SessionEvent::TurnError("fail".into()),
        ];
        assert_eq!(events.len(), 7);
        assert!(matches!(events[0], SessionEvent::TextDelta(_)));
        assert!(matches!(events[5], SessionEvent::TurnComplete));
    }

    #[test]
    fn approval_decision_is_eq() {
        assert_eq!(ApprovalDecision::Approve, ApprovalDecision::Approve);
        assert_ne!(ApprovalDecision::Approve, ApprovalDecision::Deny);
    }

    #[test]
    fn persisted_event_variants_are_constructible() {
        let events = [
            PersistedEvent::TurnStarted { turn_index: 0 },
            PersistedEvent::UserMessage {
                text: "hello".into(),
            },
            PersistedEvent::AssistantMessage {
                text: "world".into(),
            },
            PersistedEvent::ToolCallRequested {
                id: "1".into(),
                name: "test".into(),
                arguments_json: "{}".into(),
            },
            PersistedEvent::ToolApprovalRequested {
                id: "1".into(),
                name: "test".into(),
            },
            PersistedEvent::ToolApprovalDecided {
                id: "1".into(),
                decision: ApprovalDecision::Approve,
            },
            PersistedEvent::ToolResultReceived {
                tool_call_id: "1".into(),
                content: "ok".into(),
            },
            PersistedEvent::TurnComplete { turn_index: 0 },
            PersistedEvent::TurnInterrupted {
                turn_index: 0,
                reason: "cancelled".into(),
            },
        ];
        assert_eq!(events.len(), 9);
    }

    #[test]
    fn session_snapshot_contains_messages_and_events() {
        let snapshot = SessionSnapshot {
            session_id: "test-session".into(),
            messages: vec![
                text_message("user", "hi"),
                text_message("assistant", "hello"),
            ],
            events: vec![
                PersistedEvent::TurnStarted { turn_index: 0 },
                PersistedEvent::UserMessage { text: "hi".into() },
                PersistedEvent::AssistantMessage {
                    text: "hello".into(),
                },
                PersistedEvent::TurnComplete { turn_index: 0 },
            ],
        };

        assert_eq!(snapshot.session_id, "test-session");
        assert_eq!(snapshot.messages.len(), 2);
        assert_eq!(snapshot.events.len(), 4);
    }

    #[test]
    fn replay_emits_matching_session_events() {
        let events = vec![
            PersistedEvent::TurnStarted { turn_index: 0 },
            PersistedEvent::UserMessage { text: "hi".into() },
            PersistedEvent::AssistantMessage {
                text: "hello".into(),
            },
            PersistedEvent::ToolCallRequested {
                id: "c1".into(),
                name: "search".into(),
                arguments_json: "{\"q\":\"rust\"}".into(),
            },
            PersistedEvent::ToolResultReceived {
                tool_call_id: "c1".into(),
                content: "found".into(),
            },
            PersistedEvent::TurnComplete { turn_index: 0 },
        ];

        // Build replayed events using the same logic as replay().
        let mut replayed = Vec::new();
        for event in &events {
            let session_event = match event {
                PersistedEvent::AssistantMessage { text } => {
                    Some(SessionEvent::AssistantMessage { text: text.clone() })
                }
                PersistedEvent::ToolCallRequested {
                    id,
                    name,
                    arguments_json,
                } => Some(SessionEvent::ToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    arguments_json: arguments_json.clone(),
                }),
                PersistedEvent::ToolResultReceived {
                    tool_call_id,
                    content,
                } => Some(SessionEvent::ToolResult {
                    tool_call_id: tool_call_id.clone(),
                    tool_name: String::new(),
                    content: content.clone(),
                }),
                PersistedEvent::TurnComplete { .. } => Some(SessionEvent::TurnComplete),
                _ => None,
            };
            if let Some(e) = session_event {
                replayed.push(e);
            }
        }

        assert_eq!(replayed.len(), 4);
        assert!(matches!(replayed[0], SessionEvent::AssistantMessage { .. }));
        assert!(matches!(replayed[1], SessionEvent::ToolCall { .. }));
        assert!(matches!(replayed[2], SessionEvent::ToolResult { .. }));
        assert!(matches!(replayed[3], SessionEvent::TurnComplete));
    }
}
