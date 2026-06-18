//! Async tool-using LLM agents over pluggable providers.
//!
//! `ur` owns the provider-agnostic agent loop: conversation history, streaming
//! events, reasoning content, tool dispatch, and rollback if a turn fails. The
//! facade re-exports the core API, the [`tool`] macro, and provider crates
//! enabled by Cargo features. With default features, the DeepSeek provider is
//! available as [`deepseek`].
//!
//! The full public contract is maintained in the repository
//! [API specification](https://github.com/kkestell/ur/blob/main/docs/API.md).
//! Provider-specific behavior is documented by each provider, for example
//! [DeepSeek](https://github.com/kkestell/ur/blob/main/docs/DEEPSEEK.md).
//!
//! # Example
//!
//! This uses a small scripted provider so it runs without network access. Swap
//! the provider construction for a concrete provider such as
//! [`deepseek::DeepSeekClient`] to call a live model.
//!
//! ```no_run
//! use std::collections::VecDeque;
//! use std::sync::Mutex;
//!
//! use futures_util::StreamExt;
//!
//! #[ur::tool(description = "Add two integers.")]
//! async fn add(a: i64, b: i64) -> i64 {
//!     a + b
//! }
//!
//! struct ScriptedProvider {
//!     batches: Mutex<VecDeque<Vec<ur::RawEvent>>>,
//! }
//!
//! impl ScriptedProvider {
//!     fn new() -> Self {
//!         Self {
//!             batches: Mutex::new(VecDeque::from([
//!                 vec![
//!                     ur::RawEvent::ToolCallDelta {
//!                         index: 0,
//!                         id: Some("call-1".to_owned()),
//!                         name: Some("add".to_owned()),
//!                         arguments: r#"{"a":41,"b":1}"#.to_owned(),
//!                     },
//!                     ur::RawEvent::Done {
//!                         finish_reason: ur::FinishReason::ToolCalls,
//!                         usage: None,
//!                     },
//!                 ],
//!                 vec![
//!                     ur::RawEvent::TextDelta("The answer is 42.".to_owned()),
//!                     ur::RawEvent::Done {
//!                         finish_reason: ur::FinishReason::Stop,
//!                         usage: None,
//!                     },
//!                 ],
//!             ])),
//!         }
//!     }
//! }
//!
//! impl ur::Provider for ScriptedProvider {
//!     fn chat(&self, _request: &ur::Request) -> ur::BoxStream<'static, ur::Result<ur::RawEvent>> {
//!         let batch = match self.batches.lock() {
//!             Ok(mut batches) => batches.pop_front().unwrap_or_default(),
//!             Err(_) => Vec::new(),
//!         };
//!         Box::pin(futures_util::stream::iter(batch.into_iter().map(Ok)))
//!     }
//!
//!     fn model_spec(&self, _model_id: &str) -> Option<ur::ModelSpec> {
//!         None
//!     }
//! }
//!
//! # async fn run() -> ur::Result<()> {
//! let model = ur::Model::new(ScriptedProvider::new(), "scripted-model");
//! let agent = ur::Agent::new("You are concise. Use tools when useful.", model).tool(add);
//! let mut session = agent.session();
//!
//! let mut events = session.send("What is 41 + 1?");
//! while let Some(event) = events.next().await {
//!     match event? {
//!         ur::Event::TextDelta { delta } => print!("{delta}"),
//!         ur::Event::Done { .. } => break,
//!         _ => {}
//!     }
//! }
//! # Ok(())
//! # }
//! ```

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
