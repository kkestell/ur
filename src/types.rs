//! Native domain types replacing the WIT-generated types.
//!
//! These are the shared data types used across providers, session
//! management, extensions, and the CLI.

use serde::{Deserialize, Serialize};

// ── Messages ────────────────────────────────────────────────────────

/// A conversation message with a role and content parts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub parts: Vec<MessagePart>,
}

/// A piece of a message: text, tool call, or tool result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessagePart {
    Text(TextPart),
    ToolCall(ToolCall),
    ToolResult(ToolResult),
}

/// Wrapper for text content in a message part.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextPart {
    pub text: String,
}

/// An LLM-issued tool invocation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments_json: String,
    /// Opaque provider metadata echoed back on subsequent requests.
    #[serde(default)]
    pub provider_metadata_json: String,
}

/// The result of executing a tool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_call_id: String,
    pub tool_name: String,
    pub content: String,
}

// ── Completions ─────────────────────────────────────────────────────

/// Token usage statistics from an LLM completion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
}

/// A complete LLM response.
#[derive(Debug, Clone)]
pub struct Completion {
    pub message: Message,
    pub usage: Option<Usage>,
}

/// A streaming chunk from an LLM completion.
#[derive(Debug, Clone)]
pub struct CompletionChunk {
    pub delta_parts: Vec<MessagePart>,
    pub usage: Option<Usage>,
}

// ── Tools ───────────────────────────────────────────────────────────

/// Describes a tool that can be invoked by the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDescriptor {
    pub name: String,
    pub description: String,
    pub parameters_json_schema: String,
}

/// Controls how the LLM selects tools.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolChoice {
    Auto,
    None,
    Required,
    Specific(String),
}

// ── Models ──────────────────────────────────────────────────────────

/// Describes a model offered by a provider.
#[derive(Debug, Clone)]
pub struct ModelDescriptor {
    pub id: String,
    pub name: String,
    pub description: String,
    pub is_default: bool,
}

// ── Sessions ────────────────────────────────────────────────────────

/// Summary info for a persisted session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    pub created_at: String,
    pub message_count: u32,
}

/// A structured event in the session timeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionEvent {
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

/// A tool approval decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    Approve,
    Deny,
}

// ── Settings ────────────────────────────────────────────────────────

/// A key-value config setting passed to providers.
#[derive(Debug, Clone)]
pub struct ConfigSetting {
    pub key: String,
    pub value: SettingValue,
}

/// A typed setting value.
#[derive(Debug, Clone)]
pub enum SettingValue {
    Integer(i64),
    Enumeration(String),
    Boolean(bool),
    Number(f64),
    String(String),
}

/// Schema for a setting, defining its type and constraints.
#[derive(Debug, Clone)]
pub enum SettingSchema {
    Integer(SettingInteger),
    Enumeration(SettingEnum),
    Boolean(SettingBoolean),
    Number(SettingNumber),
    String(SettingString),
}

#[derive(Debug, Clone)]
pub struct SettingInteger {
    pub min: i64,
    pub max: i64,
    pub default_val: i64,
}

#[derive(Debug, Clone)]
pub struct SettingEnum {
    pub allowed: Vec<String>,
    pub default_val: String,
}

#[derive(Debug, Clone)]
pub struct SettingBoolean {
    pub default_val: bool,
}

#[derive(Debug, Clone)]
pub struct SettingNumber {
    pub min: f64,
    pub max: f64,
    pub default_val: f64,
}

#[derive(Debug, Clone)]
pub struct SettingString {
    pub default_val: String,
}

/// Describes a configurable setting exposed by a provider.
#[derive(Debug, Clone)]
pub struct SettingDescriptor {
    pub key: String,
    pub name: String,
    pub description: String,
    pub schema: SettingSchema,
    pub secret: bool,
    pub readonly: bool,
}

// ── Extension capabilities ──────────────────────────────────────────

/// Capabilities an extension can declare.
#[derive(Debug, Clone, Default)]
pub struct ExtensionCapabilities {
    pub network: bool,
    pub fs_read: bool,
    pub fs_write: bool,
}

impl ExtensionCapabilities {
    /// Parses capability strings from an extension manifest.
    #[must_use]
    pub fn from_strings(caps: &[String]) -> Self {
        let mut result = Self::default();
        for cap in caps {
            match cap.as_str() {
                "network" => result.network = true,
                "fs-read" => result.fs_read = true,
                "fs-write" => result.fs_write = true,
                _ => {}
            }
        }
        result
    }

    /// Returns capability strings for serialization.
    #[must_use]
    pub fn to_strings(&self) -> Vec<String> {
        let mut result = Vec::new();
        if self.network {
            result.push("network".into());
        }
        if self.fs_read {
            result.push("fs-read".into());
        }
        if self.fs_write {
            result.push("fs-write".into());
        }
        result
    }
}

// ── Convenience constructors ────────────────────────────────────────

impl Message {
    /// Creates a text-only message.
    #[must_use]
    pub fn text(role: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            parts: vec![MessagePart::Text(TextPart { text: text.into() })],
        }
    }
}

impl MessagePart {
    /// Returns the text content if this is a text part.
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(t) => Some(&t.text),
            _ => None,
        }
    }
}
