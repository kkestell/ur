//! User-facing facade for `ur`.
//!
//! This crate re-exports the provider-agnostic core API and conditionally
//! exposes provider crates behind feature flags.

#![forbid(unsafe_code)]

pub use ur_core::event::{Event, FinishReason, ToolOutput, Usage};
pub use ur_core::model::{ReasoningEffort, ResponseFormat, Thinking};
pub use ur_core::provider::{
    Message, MessageRole, ModelNotice, ModelSpec, Provider, RawEvent, Request, Settings, ToolCall,
};
pub use ur_core::tool::{Tool, ToolArguments, ToolSchema};
pub use ur_core::{
    Agent, BoxFuture, BoxStream, Error, EventStream, JsonError, JsonSchema, JsonValue, Model,
    Result, Session, Stream, UserMessage,
};
pub use ur_macros::tool;

#[doc(hidden)]
pub use ur_core::__rt;

#[cfg(feature = "deepseek")]
pub use ur_deepseek as deepseek;

#[cfg(test)]
mod tests {
    #[test]
    fn facade_compiles() {
        let _ = crate::ResponseFormat::Text;
    }
}
