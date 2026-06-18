//! Provider-agnostic core API for `ur`.

#![forbid(unsafe_code)]

pub mod event;
pub mod model;
pub mod provider;
pub mod tool;

pub use futures_core::Stream;
pub use schemars::JsonSchema;
pub use serde_json::{Error as JsonError, Value as JsonValue};

pub use event::EventStream;

/// A boxed, sendable future returned by asynchronous extension points.
pub type BoxFuture<'a, T> = futures_core::future::BoxFuture<'a, T>;

/// A boxed, sendable stream returned by providers.
pub type BoxStream<'a, T> = futures_core::stream::BoxStream<'a, T>;

/// The shared result type used by `ur`.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Placeholder error type; phase 2 fills in the shared error vocabulary.
#[derive(Debug, thiserror::Error)]
#[error("ur core placeholder error")]
pub struct Error;

/// Placeholder model handle.
#[derive(Clone, Debug)]
pub struct Model<P> {
    provider: P,
}

impl<P> Model<P> {
    /// Creates a placeholder model from a provider.
    pub fn new(provider: P) -> Self {
        Self { provider }
    }

    /// Returns the provider bound to this placeholder model.
    pub fn provider(&self) -> &P {
        &self.provider
    }
}

/// Placeholder agent type.
#[derive(Clone, Debug)]
pub struct Agent<P> {
    model: Model<P>,
}

impl<P> Agent<P> {
    /// Creates a placeholder agent from a model.
    pub fn new(model: Model<P>) -> Self {
        Self { model }
    }

    /// Returns the model bound to this placeholder agent.
    pub fn model(&self) -> &Model<P> {
        &self.model
    }
}

/// Placeholder session type.
#[derive(Clone, Debug)]
pub struct Session<P> {
    agent: Agent<P>,
}

impl<P> Session<P> {
    /// Creates a placeholder session from an agent.
    pub fn new(agent: Agent<P>) -> Self {
        Self { agent }
    }

    /// Returns the agent bound to this placeholder session.
    pub fn agent(&self) -> &Agent<P> {
        &self.agent
    }
}

/// Placeholder user message.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct UserMessage(String);

impl UserMessage {
    /// Creates a user message from text.
    pub fn new(content: impl Into<String>) -> Self {
        Self(content.into())
    }

    /// Returns the message content.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_are_available() {
        fn accepts_stream<S: Stream<Item = ()>>(_stream: S) {}

        let stream = futures_util::stream::empty();
        accepts_stream(stream);

        let _: JsonValue = serde_json::json!({ "ok": true });
    }
}
