//! Serde-friendly representations of WIT session event types.
//!
//! WIT-generated types don't derive Serialize/Deserialize, so we
//! convert at the boundary between WIT and JSON.

use serde::{Deserialize, Serialize};

use crate::{
    ApprovalDecision, Message, MessagePart, SessionEvent, ToolApprovalDecisionRecord,
    ToolApprovalRequest, ToolCall, TurnInterruption,
};

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SerdeSessionEvent {
    TurnStarted {
        turn_index: u32,
    },
    UserMessage {
        text: String,
    },
    LlmCompletion {
        message: SerdeMessage,
    },
    ToolResult {
        message: SerdeMessage,
    },
    ToolApprovalRequested {
        id: String,
        name: String,
    },
    ToolApprovalDecided {
        id: String,
        decision: SerdeApprovalDecision,
    },
    TurnComplete {
        turn_index: u32,
    },
    TurnInterrupted {
        turn_index: u32,
        reason: String,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SerdeApprovalDecision {
    Approve,
    Deny,
}

#[derive(Serialize, Deserialize)]
pub struct SerdeMessage {
    pub role: String,
    pub parts: Vec<SerdePart>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SerdePart {
    Text {
        text: String,
    },
    ToolCall {
        id: String,
        name: String,
        arguments_json: String,
        #[serde(default)]
        provider_metadata_json: String,
    },
    ToolResult {
        tool_call_id: String,
        tool_name: String,
        content: String,
    },
}

impl SerdeSessionEvent {
    pub fn from_wit(event: &SessionEvent) -> Self {
        match event {
            SessionEvent::TurnStarted(idx) => Self::TurnStarted { turn_index: *idx },
            SessionEvent::UserMessage(text) => Self::UserMessage { text: text.clone() },
            SessionEvent::LlmCompletion(msg) => Self::LlmCompletion {
                message: SerdeMessage::from_wit(msg),
            },
            SessionEvent::ToolResult(msg) => Self::ToolResult {
                message: SerdeMessage::from_wit(msg),
            },
            SessionEvent::ToolApprovalRequested(req) => Self::ToolApprovalRequested {
                id: req.id.clone(),
                name: req.name.clone(),
            },
            SessionEvent::ToolApprovalDecided(rec) => Self::ToolApprovalDecided {
                id: rec.id.clone(),
                decision: match rec.decision {
                    ApprovalDecision::Approve => SerdeApprovalDecision::Approve,
                    ApprovalDecision::Deny => SerdeApprovalDecision::Deny,
                },
            },
            SessionEvent::TurnComplete(idx) => Self::TurnComplete { turn_index: *idx },
            SessionEvent::TurnInterrupted(ti) => Self::TurnInterrupted {
                turn_index: ti.turn_index,
                reason: ti.reason.clone(),
            },
        }
    }

    pub fn into_wit(self) -> SessionEvent {
        match self {
            Self::TurnStarted { turn_index } => SessionEvent::TurnStarted(turn_index),
            Self::UserMessage { text } => SessionEvent::UserMessage(text),
            Self::LlmCompletion { message } => SessionEvent::LlmCompletion(message.into_wit()),
            Self::ToolResult { message } => SessionEvent::ToolResult(message.into_wit()),
            Self::ToolApprovalRequested { id, name } => {
                SessionEvent::ToolApprovalRequested(ToolApprovalRequest { id, name })
            }
            Self::ToolApprovalDecided { id, decision } => {
                SessionEvent::ToolApprovalDecided(ToolApprovalDecisionRecord {
                    id,
                    decision: match decision {
                        SerdeApprovalDecision::Approve => ApprovalDecision::Approve,
                        SerdeApprovalDecision::Deny => ApprovalDecision::Deny,
                    },
                })
            }
            Self::TurnComplete { turn_index } => SessionEvent::TurnComplete(turn_index),
            Self::TurnInterrupted { turn_index, reason } => {
                SessionEvent::TurnInterrupted(TurnInterruption { turn_index, reason })
            }
        }
    }
}

impl SerdeMessage {
    fn from_wit(msg: &Message) -> Self {
        Self {
            role: msg.role.clone(),
            parts: msg.parts.iter().map(SerdePart::from_wit).collect(),
        }
    }

    fn into_wit(self) -> Message {
        Message {
            role: self.role,
            parts: self.parts.into_iter().map(SerdePart::into_wit).collect(),
        }
    }
}

impl SerdePart {
    fn from_wit(part: &MessagePart) -> Self {
        match part {
            MessagePart::Text(text) => Self::Text { text: text.clone() },
            MessagePart::ToolCall(tc) => Self::ToolCall {
                id: tc.id.clone(),
                name: tc.name.clone(),
                arguments_json: tc.arguments_json.clone(),
                provider_metadata_json: tc.provider_metadata_json.clone(),
            },
            MessagePart::ToolResult(tr) => Self::ToolResult {
                tool_call_id: tr.tool_call_id.clone(),
                tool_name: tr.tool_name.clone(),
                content: tr.content.clone(),
            },
        }
    }

    fn into_wit(self) -> MessagePart {
        match self {
            Self::Text { text } => MessagePart::Text(text),
            Self::ToolCall {
                id,
                name,
                arguments_json,
                provider_metadata_json,
            } => MessagePart::ToolCall(ToolCall {
                id,
                name,
                arguments_json,
                provider_metadata_json,
            }),
            Self::ToolResult {
                tool_call_id,
                tool_name,
                content,
            } => MessagePart::ToolResult(crate::ToolResult {
                tool_call_id,
                tool_name,
                content,
            }),
        }
    }
}
