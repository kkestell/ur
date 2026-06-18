//! Event types produced by the agent loop.

/// Placeholder event stream.
#[derive(Debug)]
pub struct EventStream;

/// Placeholder event.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum Event {
    /// Placeholder terminal event.
    Done,
}

/// Placeholder finish reason.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum FinishReason {
    /// The model completed normally.
    Stop,
}

/// Placeholder token usage.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct Usage {
    /// Input tokens.
    pub input_tokens: u32,
    /// Output tokens.
    pub output_tokens: u32,
}

/// Placeholder tool result.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct ToolOutput {
    content: String,
}

impl ToolOutput {
    /// Creates a successful placeholder tool output.
    pub fn ok(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
        }
    }

    /// Returns the tool result content.
    pub fn content(&self) -> &str {
        &self.content
    }
}
