//! Provider seam and request/response records.

use crate::event::{FinishReason, Usage};
use crate::model::{ReasoningEffort, ResponseFormat, Thinking};
use crate::tool::{ToolArguments, ToolSchema};
use crate::{BoxStream, Result};

/// A provider backend that can drive one normalized model turn.
pub trait Provider: Send + Sync + 'static {
    /// Drives one model turn and returns the normalized provider event stream.
    fn chat(&self, request: &Request) -> BoxStream<'static, Result<RawEvent>>;

    /// Returns static facts for a known model id.
    fn model_spec(&self, model_id: &str) -> Option<ModelSpec>;

    /// Returns a static non-fatal notice for a model id, if any.
    fn model_notice(&self, model_id: &str) -> Option<ModelNotice> {
        let _ = model_id;
        None
    }
}

impl<T: Provider + ?Sized> Provider for std::sync::Arc<T> {
    fn chat(&self, request: &Request) -> BoxStream<'static, Result<RawEvent>> {
        (**self).chat(request)
    }

    fn model_spec(&self, model_id: &str) -> Option<ModelSpec> {
        (**self).model_spec(model_id)
    }

    fn model_notice(&self, model_id: &str) -> Option<ModelNotice> {
        (**self).model_notice(model_id)
    }
}

/// A provider request for a single model turn.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct Request {
    /// Provider-specific model id.
    pub model: String,
    /// Complete conversation history for the turn.
    pub messages: Vec<Message>,
    /// Registered tool schemas in request order.
    pub tools: Vec<ToolSchema>,
    /// Generation settings.
    pub settings: Settings,
}

/// A normalized provider event consumed by the agent loop.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum RawEvent {
    /// Incremental assistant text.
    TextDelta(String),
    /// Incremental reasoning text.
    ReasoningDelta(String),
    /// A partial tool-call fragment.
    ToolCallDelta {
        index: u32,
        id: Option<String>,
        name: Option<String>,
        arguments: String,
    },
    /// Terminal event for one provider turn.
    Done {
        finish_reason: FinishReason,
        usage: Option<Usage>,
    },
}

/// The role of a conversation message.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
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

/// A foundational conversation record.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct Message {
    role: MessageRole,
    content: Option<String>,
    reasoning_content: Option<String>,
    tool_calls: Vec<ToolCall>,
    tool_call_id: Option<String>,
}

impl Message {
    /// Creates a system message.
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::System,
            content: Some(content.into()),
            reasoning_content: None,
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }

    /// Creates a user message.
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::User,
            content: Some(content.into()),
            reasoning_content: None,
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }

    /// Creates an assistant message.
    pub fn assistant(
        content: Option<String>,
        reasoning_content: Option<String>,
        tool_calls: Vec<ToolCall>,
    ) -> Self {
        Self {
            role: MessageRole::Assistant,
            content,
            reasoning_content,
            tool_calls,
            tool_call_id: None,
        }
    }

    /// Creates a tool message.
    pub fn tool(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Tool,
            content: Some(content.into()),
            reasoning_content: None,
            tool_calls: Vec::new(),
            tool_call_id: Some(tool_call_id.into()),
        }
    }

    /// Returns the message role.
    pub fn role(&self) -> MessageRole {
        self.role
    }

    /// Returns the message content.
    pub fn content(&self) -> Option<&str> {
        self.content.as_deref()
    }

    /// Returns assistant reasoning content, when present.
    pub fn reasoning_content(&self) -> Option<&str> {
        self.reasoning_content.as_deref()
    }

    /// Returns assistant tool calls.
    pub fn tool_calls(&self) -> &[ToolCall] {
        &self.tool_calls
    }

    /// Returns the provider-issued tool-call id for tool messages.
    pub fn tool_call_id(&self) -> Option<&str> {
        self.tool_call_id.as_deref()
    }
}

/// A completed tool call emitted by a model.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct ToolCall {
    /// Provider-issued tool call id.
    pub id: String,
    /// Tool name.
    pub name: String,
    /// Raw tool arguments.
    pub arguments: ToolArguments,
}

impl ToolCall {
    /// Creates a tool call.
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        arguments: impl Into<ToolArguments>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            arguments: arguments.into(),
        }
    }
}

/// Static facts about a model.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct ModelSpec {
    /// Context window in tokens.
    pub context_window: u32,
    /// Maximum output in tokens.
    pub max_output: u32,
}

impl ModelSpec {
    /// Creates a model specification.
    pub fn new(context_window: u32, max_output: u32) -> Self {
        Self {
            context_window,
            max_output,
        }
    }
}

/// Static non-fatal model notice.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum ModelNotice {
    /// The model id is deprecated.
    Deprecated { message: String },
}

/// Generation settings carried verbatim to providers.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct Settings {
    /// Thinking mode.
    pub thinking: Thinking,
    /// Requested reasoning effort.
    pub reasoning_effort: Option<ReasoningEffort>,
    /// Maximum output tokens.
    pub max_tokens: Option<u32>,
    /// Stop sequences.
    pub stop: Vec<String>,
    /// Desired response format.
    pub response_format: ResponseFormat,
    /// Sampling temperature.
    pub temperature: Option<f32>,
    /// Nucleus sampling probability.
    pub top_p: Option<f32>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn message_and_tool_call_constructors_expose_provider_fields() {
        let system = Message::system("system");
        assert_eq!(system.role(), MessageRole::System);
        assert_eq!(system.content(), Some("system"));
        assert_eq!(system.reasoning_content(), None);
        assert!(system.tool_calls().is_empty());
        assert_eq!(system.tool_call_id(), None);

        let user = Message::user("hello");
        assert_eq!(user.role(), MessageRole::User);
        assert_eq!(user.content(), Some("hello"));

        let call = ToolCall::new("call-1", "add", r#"{"a":1,"b":2}"#);
        let assistant = Message::assistant(
            Some("content".to_owned()),
            Some("reasoning".to_owned()),
            vec![call.clone()],
        );
        assert_eq!(assistant.role(), MessageRole::Assistant);
        assert_eq!(assistant.content(), Some("content"));
        assert_eq!(assistant.reasoning_content(), Some("reasoning"));
        assert_eq!(assistant.tool_calls(), std::slice::from_ref(&call));
        assert_eq!(assistant.tool_call_id(), None);

        let tool = Message::tool("call-1", "3");
        assert_eq!(tool.role(), MessageRole::Tool);
        assert_eq!(tool.content(), Some("3"));
        assert_eq!(tool.reasoning_content(), None);
        assert!(tool.tool_calls().is_empty());
        assert_eq!(tool.tool_call_id(), Some("call-1"));

        assert_eq!(call.id, "call-1");
        assert_eq!(call.name, "add");
        assert_eq!(call.arguments.as_str(), r#"{"a":1,"b":2}"#);
    }

    #[test]
    fn provider_is_object_safe_behind_arc() {
        struct FakeProvider;

        impl Provider for FakeProvider {
            fn chat(&self, _request: &Request) -> BoxStream<'static, Result<RawEvent>> {
                Box::pin(futures_util::stream::empty())
            }

            fn model_spec(&self, model_id: &str) -> Option<ModelSpec> {
                (model_id == "known").then_some(ModelSpec::new(128, 16))
            }
        }

        let provider: Arc<dyn Provider> = Arc::new(FakeProvider);
        let shared = Arc::new(provider);
        assert_eq!(shared.model_spec("known"), Some(ModelSpec::new(128, 16)));
        assert_eq!(shared.model_notice("known"), None);
    }

    #[test]
    fn public_provider_records_have_required_traits() {
        fn assert_clone_debug_partial_eq<T: Clone + std::fmt::Debug + PartialEq>() {}
        fn assert_eq_hash<T: Eq + std::hash::Hash>() {}
        fn assert_send_sync_static<T: Send + Sync + 'static>() {}

        assert_clone_debug_partial_eq::<Request>();
        assert_clone_debug_partial_eq::<RawEvent>();
        assert_clone_debug_partial_eq::<MessageRole>();
        assert_clone_debug_partial_eq::<Message>();
        assert_clone_debug_partial_eq::<ToolCall>();
        assert_clone_debug_partial_eq::<ModelSpec>();
        assert_clone_debug_partial_eq::<ModelNotice>();
        assert_clone_debug_partial_eq::<Settings>();

        assert_eq_hash::<RawEvent>();
        assert_eq_hash::<MessageRole>();
        assert_eq_hash::<Message>();
        assert_eq_hash::<ToolCall>();
        assert_eq_hash::<ModelSpec>();

        assert_send_sync_static::<Request>();
        assert_send_sync_static::<RawEvent>();
        assert_send_sync_static::<Message>();
        assert_send_sync_static::<Settings>();

        let settings = Settings::default();
        assert_eq!(settings.thinking, Thinking::Default);
        assert_eq!(settings.response_format, ResponseFormat::Text);
        assert!(settings.stop.is_empty());
    }
}
