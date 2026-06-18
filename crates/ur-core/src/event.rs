//! Event types produced by the agent loop.

use std::hash::Hash;

use crate::tool::ToolArguments;

/// Opaque stream of events returned by a session.
#[derive(Debug)]
pub struct EventStream;

/// An event yielded by the provider-agnostic agent loop.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum Event {
    /// Incremental assistant text.
    TextDelta { delta: String },
    /// Incremental reasoning text.
    ReasoningDelta { delta: String },
    /// A fully assembled tool call.
    ToolCall {
        id: String,
        name: String,
        arguments: ToolArguments,
    },
    /// The result of running a tool.
    ToolResult {
        id: String,
        name: String,
        output: ToolOutput,
    },
    /// Token accounting for the most recent model turn.
    Usage { usage: Usage },
    /// Terminal completion for a whole user turn.
    Done { finish_reason: FinishReason },
}

/// Why a model turn finished.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum FinishReason {
    /// The model completed normally.
    Stop,
    /// Generation reached the token limit.
    Length,
    /// Output was withheld or truncated by a content filter.
    ContentFilter,
    /// The model emitted tool calls.
    ToolCalls,
    /// A provider-specific terminal reason.
    Other(String),
}

/// Token accounting reported by a provider.
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct Usage {
    /// Input tokens.
    pub prompt_tokens: u32,
    /// Output tokens.
    pub completion_tokens: u32,
    /// Total tokens.
    pub total_tokens: u32,
    /// Prompt tokens served from a provider-side cache, when reported.
    pub cached_prompt_tokens: Option<u32>,
    /// Reasoning tokens, when reported.
    pub reasoning_tokens: Option<u32>,
}

/// The public event form of a tool's output.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Deserialize, serde::Serialize),
    serde(tag = "status", content = "content", rename_all = "snake_case")
)]
pub enum ToolOutput {
    /// The tool completed successfully.
    Ok(String),
    /// The tool returned an error message.
    Err(String),
}

impl ToolOutput {
    /// Converts a tool result into its event representation.
    pub fn from_result(output: std::result::Result<String, String>) -> Self {
        match output {
            Ok(content) => Self::Ok(content),
            Err(content) => Self::Err(content),
        }
    }

    /// Borrows this output as a result.
    pub fn as_result(&self) -> std::result::Result<&str, &str> {
        match self {
            Self::Ok(content) => Ok(content),
            Self::Err(content) => Err(content),
        }
    }

    /// Returns the tool result content.
    pub fn content(&self) -> &str {
        match self {
            Self::Ok(content) | Self::Err(content) => content,
        }
    }
}

impl From<std::result::Result<String, String>> for ToolOutput {
    fn from(output: std::result::Result<String, String>) -> Self {
        Self::from_result(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_output_maps_results_and_borrows_content() {
        let ok = ToolOutput::from_result(Ok("value".to_owned()));
        assert_eq!(ok.as_result(), Ok("value"));
        assert_eq!(ok.content(), "value");

        let err = ToolOutput::from_result(Err("failed".to_owned()));
        assert_eq!(err.as_result(), Err("failed"));
        assert_eq!(err.content(), "failed");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn tool_output_serializes_to_documented_shape() {
        assert_eq!(
            serde_json::to_value(ToolOutput::Ok("value".to_owned())).unwrap(),
            serde_json::json!({ "status": "ok", "content": "value" })
        );
        assert_eq!(
            serde_json::to_value(ToolOutput::Err("failed".to_owned())).unwrap(),
            serde_json::json!({ "status": "err", "content": "failed" })
        );
    }

    #[test]
    fn public_event_types_have_expected_traits() {
        fn assert_common<T: Clone + std::fmt::Debug + PartialEq + Send + Sync + 'static>() {}
        fn assert_eq_hash<T: Eq + Hash>() {}

        assert_common::<Event>();
        assert_common::<ToolOutput>();
        assert_common::<FinishReason>();
        assert_common::<Usage>();
        assert_eq_hash::<ToolOutput>();
        assert_eq_hash::<FinishReason>();
        assert_eq_hash::<Usage>();
        assert_eq!(Usage::default().total_tokens, 0);
    }
}
