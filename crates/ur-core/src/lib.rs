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

/// The shared error vocabulary used by providers and the agent loop.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Authentication failed because credentials are missing or invalid.
    #[error("authentication failed")]
    Auth,
    /// The account has insufficient balance or quota.
    #[error("insufficient funds")]
    InsufficientFunds,
    /// The provider rejected the request as malformed.
    #[error("bad request: {message}")]
    BadRequest { message: String },
    /// The request was well-formed but a parameter value was rejected.
    #[error("invalid params: {message}")]
    InvalidParams { message: String },
    /// A rate or concurrency limit was reached.
    #[error("rate limited")]
    RateLimited {
        retry_after: Option<std::time::Duration>,
    },
    /// A retryable server-side failure, or an otherwise-unmapped provider status.
    #[error("server error {status}: {message}")]
    Server { status: u16, message: String },
    /// The connection or transport layer failed.
    #[error("transport error: {0}")]
    Transport(#[source] Box<dyn std::error::Error + Send + Sync>),
    /// A response or stream chunk could not be decoded.
    #[error("decode error while {context}: {source}")]
    Decode {
        context: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    /// A client or model setting was invalid before any request was sent.
    #[error("config error: {message}")]
    Config { message: String },
}

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

/// A text user message.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct UserMessage {
    content: String,
}

impl UserMessage {
    /// Returns the message content.
    pub fn as_str(&self) -> &str {
        &self.content
    }
}

impl From<&str> for UserMessage {
    fn from(content: &str) -> Self {
        Self {
            content: content.to_owned(),
        }
    }
}

impl From<String> for UserMessage {
    fn from(content: String) -> Self {
        Self { content }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error as StdError;
    use std::hash::Hash;

    #[test]
    fn aliases_are_available() {
        fn accepts_stream<S: Stream<Item = ()>>(_stream: S) {}

        let stream = futures_util::stream::empty();
        accepts_stream(stream);

        let _: JsonValue = serde_json::json!({ "ok": true });
    }

    #[test]
    fn error_sources_are_exposed_only_for_wrapped_errors() {
        let transport = Error::Transport(Box::new(std::io::Error::other("offline")));
        assert_eq!(transport.source().unwrap().to_string(), "offline");

        let decode = Error::Decode {
            context: "reading chunk".to_owned(),
            source: Box::new(serde_json::from_str::<JsonValue>("not json").unwrap_err()),
        };
        assert!(decode.source().is_some());

        assert!(Error::Auth.source().is_none());
        assert!(Error::InsufficientFunds.source().is_none());
        assert!(
            Error::BadRequest {
                message: "bad".to_owned()
            }
            .source()
            .is_none()
        );
        assert!(
            Error::InvalidParams {
                message: "bad".to_owned()
            }
            .source()
            .is_none()
        );
        assert!(Error::RateLimited { retry_after: None }.source().is_none());
        assert!(
            Error::Server {
                status: 500,
                message: "down".to_owned()
            }
            .source()
            .is_none()
        );
        assert!(
            Error::Config {
                message: "missing api key".to_owned()
            }
            .source()
            .is_none()
        );
    }

    #[test]
    fn user_message_conversions_and_traits() {
        fn assert_traits<T: Clone + std::fmt::Debug + Eq + Hash + Send + Sync + 'static>() {}
        assert_traits::<UserMessage>();

        let borrowed = UserMessage::from("hello");
        let owned = UserMessage::from(String::from("hello"));

        assert_eq!(borrowed, owned);
        assert_eq!(borrowed.as_str(), "hello");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn user_message_serializes_with_content_field() {
        let message = UserMessage::from("hello");
        let json = serde_json::to_value(&message).unwrap();
        assert_eq!(json, serde_json::json!({ "content": "hello" }));

        let round_trip: UserMessage = serde_json::from_value(json).unwrap();
        assert_eq!(round_trip.as_str(), "hello");
    }
}
