//! Session lifecycle and turn execution.
//!
//! `UrSession` owns a persisted conversation session and drives the
//! agent turn state machine.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Result, bail};
use tracing::{debug, info};

use crate::config::UserConfig;
use crate::manifest::WorkspaceManifest;
use crate::model;
use crate::provider;
use crate::providers::compaction::StubCompactionProvider;
use crate::providers::session_jsonl::JsonlSessionProvider;
use crate::providers::{CompactionProvider, LlmProvider, SessionProvider};
use crate::types::{
    self, Completion, CompletionChunk, Message, MessagePart, TextPart, ToolCall, ToolChoice,
    ToolDescriptor, ToolResult, Usage,
};

/// A structured event emitted during turn execution.
#[derive(Debug, Clone)]
pub enum SessionEvent {
    TextDelta(String),
    ToolCall {
        id: String,
        name: String,
        arguments_json: String,
    },
    ToolResult {
        tool_call_id: String,
        tool_name: String,
        content: String,
    },
    AssistantMessage {
        text: String,
    },
    ApprovalRequired {
        id: String,
        tool_name: String,
        arguments_json: String,
    },
    TurnComplete,
    TurnError(String),
}

/// Client response to an approval request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalDecision {
    Approve,
    Deny,
}

/// A persisted event in the session timeline.
#[derive(Debug, Clone)]
pub enum PersistedEvent {
    TurnStarted {
        turn_index: u32,
    },
    UserMessage {
        text: String,
    },
    LlmCompletion {
        message: Message,
    },
    ToolResult {
        message: Message,
    },
    ToolApprovalRequested {
        id: String,
        name: String,
    },
    ToolApprovalDecided {
        id: String,
        decision: ApprovalDecision,
    },
    TurnComplete {
        turn_index: u32,
    },
    TurnInterrupted {
        turn_index: u32,
        reason: String,
    },
}

/// A snapshot of session state sufficient to restore client UI.
#[derive(Debug, Clone)]
pub struct SessionSnapshot {
    pub session_id: String,
    pub messages: Vec<Message>,
    pub events: Vec<PersistedEvent>,
}

/// Session-scoped coordinator for turn execution.
pub struct UrSession {
    llm_providers: Vec<Arc<dyn LlmProvider>>,
    session_provider: Arc<dyn SessionProvider>,
    compaction_provider: Arc<dyn CompactionProvider>,
    config: UserConfig,
    session_id: String,
    events: Vec<PersistedEvent>,
    persisted_event_count: usize,
    turn_count: u32,
    /// Tool handlers registered by Lua extensions.
    tool_handlers: Vec<(
        ToolDescriptor,
        Arc<dyn Fn(&str) -> Result<String> + Send + Sync>,
    )>,
}

impl UrSession {
    /// Creates a session by loading existing events from the session provider.
    pub(crate) fn open(
        llm_providers: Vec<Arc<dyn LlmProvider>>,
        session_provider: Arc<dyn SessionProvider>,
        compaction_provider: Arc<dyn CompactionProvider>,
        config: UserConfig,
        session_id: &str,
        tool_handlers: Vec<(
            ToolDescriptor,
            Arc<dyn Fn(&str) -> Result<String> + Send + Sync>,
        )>,
    ) -> Result<Self> {
        let stored_events = session_provider.load_session(session_id)?;
        let events: Vec<PersistedEvent> = stored_events
            .into_iter()
            .map(types_event_to_persisted)
            .collect();
        let persisted_event_count = events.len();

        info!(
            session_id,
            count = persisted_event_count,
            state = if events.is_empty() {
                "fresh"
            } else {
                "existing"
            },
            "session loaded"
        );

        Ok(Self {
            llm_providers,
            session_provider,
            compaction_provider,
            config,
            session_id: session_id.to_owned(),
            events,
            persisted_event_count,
            turn_count: 0,
            tool_handlers,
        })
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.session_id
    }

    #[must_use]
    pub fn messages_for_llm(&self) -> Vec<Message> {
        messages_from_events(&self.events)
    }

    /// Runs a single agent turn with a user message.
    pub fn run_turn(
        &mut self,
        user_message: &str,
        mut on_event: impl FnMut(SessionEvent) -> Option<ApprovalDecision>,
    ) -> Result<()> {
        let turn_index = self.turn_count;
        self.turn_count += 1;
        self.events.push(PersistedEvent::TurnStarted { turn_index });

        debug!(text = user_message, "adding user message");
        self.events.push(PersistedEvent::UserMessage {
            text: user_message.to_owned(),
        });

        // Resolve role and find LLM provider.
        let provider_models = model::collect_provider_models(
            &self
                .llm_providers
                .iter()
                .map(|p| p.as_ref())
                .collect::<Vec<_>>(),
        );
        let (provider_id, model_id) =
            model::resolve_role(&self.config, "default", &provider_models)?;
        info!(%provider_id, %model_id, "resolved role \"default\"");

        let llm = self
            .llm_providers
            .iter()
            .find(|p| p.provider_id() == provider_id)
            .ok_or_else(|| anyhow::anyhow!("no provider with id \"{provider_id}\""))?
            .clone();

        // Collect tools from extensions.
        let tools: Vec<ToolDescriptor> =
            self.tool_handlers.iter().map(|(d, _)| d.clone()).collect();

        // Get settings for this model.
        let descriptors = llm.list_settings();
        let config_settings =
            self.config
                .settings_for(llm.provider_id(), &model_id, &descriptors)?;

        // First LLM completion.
        let messages = self.messages_for_llm();
        info!(messages = messages.len(), "calling LLM streaming");
        let completion = stream_completion(
            &*llm,
            &messages,
            &model_id,
            &config_settings,
            &tools,
            &mut on_event,
        )?;

        let tool_calls = extract_tool_calls(&completion.message);
        if tool_calls.is_empty() {
            let text = extract_text(&completion.message);
            on_event(SessionEvent::AssistantMessage { text });
        } else {
            for tc in &tool_calls {
                on_event(SessionEvent::ToolCall {
                    id: tc.id.clone(),
                    name: tc.name.clone(),
                    arguments_json: tc.arguments_json.clone(),
                });
            }
        }
        self.events.push(PersistedEvent::LlmCompletion {
            message: completion.message.clone(),
        });

        // Tool dispatch.
        if !tool_calls.is_empty() {
            self.dispatch_tool_calls(&tool_calls, &mut on_event)?;

            // Second LLM completion with tool results.
            let messages = self.messages_for_llm();
            info!(
                messages = messages.len(),
                "calling LLM streaming (with tool results)"
            );
            let completion2 = stream_completion(
                &*llm,
                &messages,
                &model_id,
                &config_settings,
                &tools,
                &mut on_event,
            )?;
            let text = extract_text(&completion2.message);
            on_event(SessionEvent::AssistantMessage { text });
            self.events.push(PersistedEvent::LlmCompletion {
                message: completion2.message,
            });
        }

        self.persist_and_compact()?;

        self.events
            .push(PersistedEvent::TurnComplete { turn_index });
        on_event(SessionEvent::TurnComplete);
        info!("turn complete");
        Ok(())
    }

    #[must_use]
    pub fn snapshot(&self) -> SessionSnapshot {
        SessionSnapshot {
            session_id: self.session_id.clone(),
            messages: self.messages_for_llm(),
            events: self.events.clone(),
        }
    }

    pub fn replay(&self, mut on_event: impl FnMut(SessionEvent)) {
        for event in &self.events {
            let session_event = match event {
                PersistedEvent::LlmCompletion { message } => {
                    let tcs = extract_tool_calls(message);
                    if tcs.is_empty() {
                        Some(SessionEvent::AssistantMessage {
                            text: extract_text(message),
                        })
                    } else {
                        for tc in &tcs {
                            on_event(SessionEvent::ToolCall {
                                id: tc.id.clone(),
                                name: tc.name.clone(),
                                arguments_json: tc.arguments_json.clone(),
                            });
                        }
                        None
                    }
                }
                PersistedEvent::ToolResult { message } => {
                    message.parts.iter().find_map(|p| match p {
                        MessagePart::ToolResult(tr) => Some(SessionEvent::ToolResult {
                            tool_call_id: tr.tool_call_id.clone(),
                            tool_name: tr.tool_name.clone(),
                            content: tr.content.clone(),
                        }),
                        _ => None,
                    })
                }
                PersistedEvent::ToolApprovalRequested { id, name } => {
                    Some(SessionEvent::ApprovalRequired {
                        id: id.clone(),
                        tool_name: name.clone(),
                        arguments_json: String::new(),
                    })
                }
                PersistedEvent::TurnComplete { .. } => Some(SessionEvent::TurnComplete),
                PersistedEvent::TurnInterrupted { reason, .. } => {
                    Some(SessionEvent::TurnError(reason.clone()))
                }
                PersistedEvent::TurnStarted { .. }
                | PersistedEvent::UserMessage { .. }
                | PersistedEvent::ToolApprovalDecided { .. } => None,
            };

            if let Some(e) = session_event {
                on_event(e);
            }
        }
    }

    fn dispatch_tool_calls(
        &mut self,
        tool_calls: &[&ToolCall],
        on_event: &mut impl FnMut(SessionEvent) -> Option<ApprovalDecision>,
    ) -> Result<()> {
        for tc in tool_calls {
            info!(tool = %tc.name, "dispatching tool");

            let handler = self
                .tool_handlers
                .iter()
                .find(|(d, _)| d.name == tc.name)
                .map(|(_, h)| h.clone());

            let result_content = if let Some(handler) = handler {
                match handler(&tc.arguments_json) {
                    Ok(result) => result,
                    Err(e) => format!("Error: {e}"),
                }
            } else {
                format!("Error: no handler for tool {:?}", tc.name)
            };

            debug!(tool = %tc.name, %result_content, "tool result");

            let msg = Message {
                role: "tool".into(),
                parts: vec![MessagePart::ToolResult(ToolResult {
                    tool_call_id: tc.id.clone(),
                    tool_name: tc.name.clone(),
                    content: result_content.clone(),
                })],
            };

            on_event(SessionEvent::ToolResult {
                tool_call_id: tc.id.clone(),
                tool_name: tc.name.clone(),
                content: result_content,
            });
            self.events
                .push(PersistedEvent::ToolResult { message: msg });
        }
        Ok(())
    }

    fn persist_and_compact(&mut self) -> Result<()> {
        let new_events = &self.events[self.persisted_event_count..];
        debug!(
            count = new_events.len(),
            session_id = self.session_id,
            "appending events to session"
        );
        for event in new_events {
            let types_event = persisted_to_types_event(event);
            self.session_provider
                .append_session(&self.session_id, &types_event)?;
        }
        self.persisted_event_count = self.events.len();

        let messages = self.messages_for_llm();
        info!(count = messages.len(), "compacting messages");
        let compacted = self.compaction_provider.compact(&messages)?;
        info!(
            count = compacted.len(),
            result = if compacted.len() == messages.len() {
                "unchanged"
            } else {
                "compacted"
            },
            "compaction complete"
        );

        Ok(())
    }
}

// --- Helpers ---

fn extract_text(msg: &Message) -> String {
    msg.parts.iter().filter_map(|p| p.as_text()).collect()
}

fn extract_tool_calls(msg: &Message) -> Vec<&ToolCall> {
    msg.parts
        .iter()
        .filter_map(|p| match p {
            MessagePart::ToolCall(tc) => Some(tc),
            _ => None,
        })
        .collect()
}

fn stream_completion(
    llm: &dyn LlmProvider,
    messages: &[Message],
    model_id: &str,
    settings: &[crate::types::ConfigSetting],
    tools: &[ToolDescriptor],
    on_event: &mut impl FnMut(SessionEvent) -> Option<ApprovalDecision>,
) -> Result<Completion> {
    let mut parts: Vec<MessagePart> = Vec::new();
    let mut usage = None;

    llm.complete(
        messages,
        model_id,
        settings,
        tools,
        None,
        &mut |chunk: CompletionChunk| {
            for dp in &chunk.delta_parts {
                match dp {
                    MessagePart::Text(text_part) => {
                        on_event(SessionEvent::TextDelta(text_part.text.clone()));
                        if let Some(MessagePart::Text(existing)) = parts.last_mut() {
                            existing.text.push_str(&text_part.text);
                        } else {
                            parts.push(MessagePart::Text(text_part.clone()));
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
            if chunk.usage.is_some() {
                usage = chunk.usage;
            }
        },
    )?;

    Ok(Completion {
        message: Message {
            role: "assistant".into(),
            parts,
        },
        usage,
    })
}

fn messages_from_events(events: &[PersistedEvent]) -> Vec<Message> {
    events
        .iter()
        .filter_map(|e| match e {
            PersistedEvent::UserMessage { text } => Some(Message::text("user", text.as_str())),
            PersistedEvent::LlmCompletion { message } | PersistedEvent::ToolResult { message } => {
                Some(message.clone())
            }
            _ => None,
        })
        .collect()
}

/// Converts a `types::SessionEvent` (from storage) to internal `PersistedEvent`.
fn types_event_to_persisted(e: types::SessionEvent) -> PersistedEvent {
    match e {
        types::SessionEvent::TurnStarted { turn_index } => {
            PersistedEvent::TurnStarted { turn_index }
        }
        types::SessionEvent::UserMessage { text } => PersistedEvent::UserMessage { text },
        types::SessionEvent::LlmCompletion { message } => PersistedEvent::LlmCompletion { message },
        types::SessionEvent::ToolResult { message } => PersistedEvent::ToolResult { message },
        types::SessionEvent::ToolApprovalRequested { id, name } => {
            PersistedEvent::ToolApprovalRequested { id, name }
        }
        types::SessionEvent::ToolApprovalDecided { id, decision } => {
            PersistedEvent::ToolApprovalDecided {
                id,
                decision: match decision {
                    types::ApprovalDecision::Approve => ApprovalDecision::Approve,
                    types::ApprovalDecision::Deny => ApprovalDecision::Deny,
                },
            }
        }
        types::SessionEvent::TurnComplete { turn_index } => {
            PersistedEvent::TurnComplete { turn_index }
        }
        types::SessionEvent::TurnInterrupted { turn_index, reason } => {
            PersistedEvent::TurnInterrupted { turn_index, reason }
        }
    }
}

/// Converts internal `PersistedEvent` to `types::SessionEvent` (for storage).
fn persisted_to_types_event(e: &PersistedEvent) -> types::SessionEvent {
    match e {
        PersistedEvent::TurnStarted { turn_index } => types::SessionEvent::TurnStarted {
            turn_index: *turn_index,
        },
        PersistedEvent::UserMessage { text } => {
            types::SessionEvent::UserMessage { text: text.clone() }
        }
        PersistedEvent::LlmCompletion { message } => types::SessionEvent::LlmCompletion {
            message: message.clone(),
        },
        PersistedEvent::ToolResult { message } => types::SessionEvent::ToolResult {
            message: message.clone(),
        },
        PersistedEvent::ToolApprovalRequested { id, name } => {
            types::SessionEvent::ToolApprovalRequested {
                id: id.clone(),
                name: name.clone(),
            }
        }
        PersistedEvent::ToolApprovalDecided { id, decision } => {
            types::SessionEvent::ToolApprovalDecided {
                id: id.clone(),
                decision: match decision {
                    ApprovalDecision::Approve => types::ApprovalDecision::Approve,
                    ApprovalDecision::Deny => types::ApprovalDecision::Deny,
                },
            }
        }
        PersistedEvent::TurnComplete { turn_index } => types::SessionEvent::TurnComplete {
            turn_index: *turn_index,
        },
        PersistedEvent::TurnInterrupted { turn_index, reason } => {
            types::SessionEvent::TurnInterrupted {
                turn_index: *turn_index,
                reason: reason.clone(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_message(role: &str, text: &str) -> Message {
        Message::text(role, text)
    }

    fn tool_call_message(tool_call_id: &str, tool_name: &str) -> Message {
        Message {
            role: "assistant".into(),
            parts: vec![MessagePart::ToolCall(ToolCall {
                id: tool_call_id.into(),
                name: tool_name.into(),
                arguments_json: "{\"city\":\"Austin\"}".into(),
                provider_metadata_json: String::new(),
            })],
        }
    }

    fn tool_result_message(tool_call_id: &str, tool_name: &str) -> Message {
        Message {
            role: "tool".into(),
            parts: vec![MessagePart::ToolResult(ToolResult {
                tool_call_id: tool_call_id.into(),
                tool_name: tool_name.into(),
                content: "{\"temperature_f\":72}".into(),
            })],
        }
    }

    #[test]
    fn messages_for_llm_derives_from_events() {
        let events = [
            PersistedEvent::TurnStarted { turn_index: 0 },
            PersistedEvent::UserMessage {
                text: "Hello".into(),
            },
            PersistedEvent::LlmCompletion {
                message: text_message("assistant", "Hi there"),
            },
            PersistedEvent::TurnComplete { turn_index: 0 },
        ];

        let messages = messages_from_events(&events);
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(extract_text(&messages[0]), "Hello");
        assert_eq!(messages[1].role, "assistant");
        assert_eq!(extract_text(&messages[1]), "Hi there");
    }

    #[test]
    fn messages_for_llm_includes_tool_turn() {
        let events = [
            PersistedEvent::UserMessage {
                text: "Weather?".into(),
            },
            PersistedEvent::LlmCompletion {
                message: tool_call_message("call-1", "get_weather"),
            },
            PersistedEvent::ToolResult {
                message: tool_result_message("call-1", "get_weather"),
            },
            PersistedEvent::LlmCompletion {
                message: text_message("assistant", "It is 72F."),
            },
        ];

        let messages = messages_from_events(&events);
        assert_eq!(messages.len(), 4);
        assert_eq!(messages[0].role, "user");
        assert!(matches!(&messages[1].parts[0], MessagePart::ToolCall(tc) if tc.id == "call-1"));
        assert!(
            matches!(&messages[2].parts[0], MessagePart::ToolResult(tr) if tr.tool_call_id == "call-1")
        );
        assert_eq!(extract_text(&messages[3]), "It is 72F.");
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
}
