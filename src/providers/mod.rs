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

/// Trait for LLM completion providers.
pub trait LlmProvider: Send + Sync {
    fn provider_id(&self) -> &'static str;
    fn list_models(&self) -> Vec<ModelDescriptor>;
    fn list_settings(&self) -> Vec<SettingDescriptor>;

    /// Runs a streaming completion. Calls `on_chunk` for each streamed chunk.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    fn complete(
        &self,
        messages: &[Message],
        model_id: &str,
        settings: &[ConfigSetting],
        tools: &[ToolDescriptor],
        tool_choice: Option<&ToolChoice>,
        on_chunk: &mut dyn FnMut(CompletionChunk),
    ) -> Result<Completion>;
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
