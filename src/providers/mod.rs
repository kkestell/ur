//! Native provider implementations replacing WASM extensions.
//!
//! Providers for LLM completion, session storage, and compaction are
//! now native Rust modules in the host process.

pub mod compaction;
pub mod google;
pub mod openrouter;
pub mod session_jsonl;

use anyhow::Result;

use crate::types::{
    Completion, CompletionChunk, ConfigSetting, Message, ModelDescriptor, SessionEvent,
    SessionInfo, SettingDescriptor, ToolChoice, ToolDescriptor,
};

/// Concrete enum dispatch for LLM completion providers.
///
/// Replaces the former `dyn LlmProvider` trait object. Methods that hit
/// the network (`list_models`, `list_settings`, `complete`) are async.
#[expect(
    missing_debug_implementations,
    reason = "OpenRouterProvider contains async RwLocks that are not Debug-friendly"
)]
pub enum LlmProvider {
    Google(google::GoogleProvider),
    OpenRouter(openrouter::OpenRouterProvider),
}

impl LlmProvider {
    pub fn provider_id(&self) -> &'static str {
        match self {
            Self::Google(p) => p.provider_id(),
            Self::OpenRouter(p) => p.provider_id(),
        }
    }

    pub async fn list_models(&self) -> Vec<ModelDescriptor> {
        match self {
            Self::Google(p) => p.list_models(),
            Self::OpenRouter(p) => p.list_models().await,
        }
    }

    pub async fn list_settings(&self) -> Vec<SettingDescriptor> {
        match self {
            Self::Google(p) => p.list_settings(),
            Self::OpenRouter(p) => p.list_settings().await,
        }
    }

    /// Runs a streaming completion. Calls `on_chunk` for each streamed chunk.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub async fn complete(
        &self,
        messages: &[Message],
        model_id: &str,
        settings: &[ConfigSetting],
        tools: &[ToolDescriptor],
        tool_choice: Option<&ToolChoice>,
        on_chunk: &mut (impl FnMut(CompletionChunk) + Send),
    ) -> Result<Completion> {
        match self {
            Self::Google(p) => {
                p.stream_completion_async(
                    messages,
                    model_id,
                    settings,
                    tools,
                    tool_choice,
                    on_chunk,
                )
                .await
            }
            Self::OpenRouter(p) => {
                p.complete_async(messages, model_id, settings, tools, tool_choice, on_chunk)
                    .await
            }
        }
    }
}

/// Trait for session persistence providers.
pub trait SessionProvider: Send + Sync {
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    fn load_session(&self, session_id: &str) -> Result<Vec<SessionEvent>>;

    /// # Errors
    ///
    /// Returns an error if the operation fails.
    fn append_session(&self, session_id: &str, event: &SessionEvent) -> Result<()>;

    /// Replaces all events for a session (used after compaction).
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    fn replace_session(&self, session_id: &str, events: &[SessionEvent]) -> Result<()>;

    /// # Errors
    ///
    /// Returns an error if the operation fails.
    fn list_sessions(&self) -> Result<Vec<SessionInfo>>;
}

/// Trait for message compaction providers.
pub trait CompactionProvider: Send + Sync {
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    fn compact(&self, messages: &[Message]) -> Result<Vec<Message>>;
}
