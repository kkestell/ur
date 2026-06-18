//! Provider seam and request/response records.

/// Placeholder provider trait.
pub trait Provider: Send + Sync + 'static {}

impl<T> Provider for std::sync::Arc<T> where T: Provider + ?Sized {}

/// Placeholder provider request.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct Request;

/// Placeholder normalized provider event.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum RawEvent {
    /// Placeholder terminal event.
    Done,
}

/// Placeholder message role.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum MessageRole {
    /// System message.
    System,
    /// User message.
    User,
    /// Assistant message.
    Assistant,
    /// Tool message.
    Tool,
}

/// Placeholder conversation message.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct Message {
    role: MessageRole,
    content: String,
}

impl Message {
    /// Creates a placeholder message.
    pub fn new(role: MessageRole, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
        }
    }

    /// Returns the message role.
    pub fn role(&self) -> MessageRole {
        self.role
    }

    /// Returns the message content.
    pub fn content(&self) -> &str {
        &self.content
    }
}

/// Placeholder tool call.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct ToolCall {
    /// Provider-issued tool call id.
    pub id: String,
    /// Tool name.
    pub name: String,
}

/// Placeholder model specification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct ModelSpec {
    /// Context window in tokens.
    pub context_window: u32,
    /// Maximum output in tokens.
    pub max_output: u32,
}

/// Placeholder model notice.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum ModelNotice {
    /// The model id is deprecated.
    Deprecated { message: String },
}

/// Placeholder generation settings.
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct Settings;
