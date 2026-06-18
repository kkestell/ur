//! Provider-agnostic core API for `ur`.

#![forbid(unsafe_code)]

pub mod event;
pub mod model;
pub mod provider;
pub mod tool;

use std::collections::HashSet;
use std::sync::Arc;

pub use futures_core::Stream;
pub use schemars::JsonSchema;
pub use serde_json::{Error as JsonError, Value as JsonValue};

#[doc(hidden)]
pub mod __rt {
    //! Plumbing referenced by code generated from `#[ur::tool]`. Not a stable
    //! public API; the macro expands to paths under `::ur::__rt`.
    pub use ::schemars;
    pub use ::serde;
    pub use ::serde_json;
}

pub use event::EventStream;
use event::StreamTool;
use provider::{Message, ModelNotice, ModelSpec, Provider, Settings};
use tool::{Tool, ToolSchema};

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

/// A provider-bound model handle and its generation settings.
pub struct Model<P: Provider> {
    provider: Arc<P>,
    id: String,
    spec: Option<ModelSpec>,
    settings: Settings,
}

impl<P: Provider> Model<P> {
    /// Binds a provider to a model id and resolves static catalog metadata.
    pub fn new(provider: P, model_id: impl Into<String>) -> Self {
        let id = model_id.into();
        let spec = provider.model_spec(&id);
        if let Some(ModelNotice::Deprecated { message }) = provider.model_notice(&id) {
            tracing::warn!(model = %id, "{message}");
        }

        Self {
            provider: Arc::new(provider),
            id,
            spec,
            settings: Settings::default(),
        }
    }

    /// Returns the provider's model id.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the provider catalog context window, if this id is known.
    pub fn context_window(&self) -> Option<u32> {
        self.spec.map(|spec| spec.context_window)
    }

    /// Returns the provider catalog max output, if this id is known.
    pub fn max_output(&self) -> Option<u32> {
        self.spec.map(|spec| spec.max_output)
    }

    /// Sets thinking mode for future requests.
    pub fn thinking(mut self, mode: model::Thinking) -> Self {
        self.settings.thinking = mode;
        self
    }

    /// Sets reasoning effort for future requests.
    pub fn reasoning_effort(mut self, effort: model::ReasoningEffort) -> Self {
        self.settings.reasoning_effort = Some(effort);
        self
    }

    /// Sets max output tokens for future requests.
    pub fn max_tokens(mut self, n: u32) -> Self {
        self.settings.max_tokens = Some(n);
        self
    }

    /// Sets sampling temperature for future requests.
    pub fn temperature(mut self, t: f32) -> Self {
        self.settings.temperature = Some(t);
        self
    }

    /// Sets nucleus sampling probability for future requests.
    pub fn top_p(mut self, p: f32) -> Self {
        self.settings.top_p = Some(p);
        self
    }

    /// Sets stop sequences for future requests.
    pub fn stop(mut self, seqs: impl IntoIterator<Item = String>) -> Self {
        self.settings.stop = seqs.into_iter().collect();
        self
    }

    /// Sets desired response format for future requests.
    pub fn response_format(mut self, fmt: model::ResponseFormat) -> Self {
        self.settings.response_format = fmt;
        self
    }

    fn validate_settings(&self) -> Result<()> {
        let Some(max_tokens) = self.settings.max_tokens else {
            return Ok(());
        };

        if max_tokens == 0 {
            return Err(Error::Config {
                message: "max_tokens must be at least 1".to_owned(),
            });
        }

        if let Some(max_output) = self.max_output()
            && max_tokens > max_output
        {
            return Err(Error::Config {
                message: format!("max_tokens {max_tokens} exceeds model max_output {max_output}"),
            });
        }

        Ok(())
    }
}

impl<P: Provider> Clone for Model<P> {
    fn clone(&self) -> Self {
        Self {
            provider: Arc::clone(&self.provider),
            id: self.id.clone(),
            spec: self.spec,
            settings: self.settings.clone(),
        }
    }
}

impl<P: Provider> std::fmt::Debug for Model<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Model")
            .field("id", &self.id)
            .field("context_window", &self.context_window())
            .field("max_output", &self.max_output())
            .finish_non_exhaustive()
    }
}

struct RegisteredTool {
    tool: Arc<dyn Tool>,
    schema: ToolSchema,
}

impl Clone for RegisteredTool {
    fn clone(&self) -> Self {
        Self {
            tool: Arc::clone(&self.tool),
            schema: self.schema.clone(),
        }
    }
}

/// A reusable agent definition with a system prompt, model, and tool set.
pub struct Agent<P: Provider> {
    system_prompt: String,
    model: Model<P>,
    tools: Vec<RegisteredTool>,
}

impl<P: Provider> Agent<P> {
    /// Creates an agent from a system prompt and model.
    pub fn new(system_prompt: impl Into<String>, model: Model<P>) -> Self {
        Self {
            system_prompt: system_prompt.into(),
            model,
            tools: Vec::new(),
        }
    }

    /// Registers one tool.
    pub fn tool<T: Tool>(mut self, tool: T) -> Self {
        let schema = tool.schema();
        self.tools.push(RegisteredTool {
            tool: Arc::new(tool),
            schema,
        });
        self
    }

    /// Registers many tools in iterator order.
    pub fn tools<T, I>(self, tools: I) -> Self
    where
        T: Tool,
        I: IntoIterator<Item = T>,
    {
        tools.into_iter().fold(self, Self::tool)
    }

    /// Starts a fresh independent conversation.
    pub fn session(&self) -> Session<P> {
        Session {
            agent: self.clone(),
            history: vec![Message::system(self.system_prompt.clone())],
        }
    }

    fn tool_schemas(&self) -> Result<Vec<ToolSchema>> {
        let mut names = HashSet::new();
        let mut schemas = Vec::with_capacity(self.tools.len());

        for registered in &self.tools {
            let runtime_name = registered.tool.name();
            validate_tool_name(runtime_name)?;
            validate_tool_name(&registered.schema.name)?;
            debug_assert!(self.tool_by_name(runtime_name).is_some());

            if runtime_name != registered.schema.name {
                return Err(Error::Config {
                    message: format!(
                        "tool name '{}' does not match schema name '{}'",
                        runtime_name, registered.schema.name
                    ),
                });
            }

            if !names.insert(registered.schema.name.as_str()) {
                return Err(Error::Config {
                    message: format!("duplicate tool name '{}'", registered.schema.name),
                });
            }

            schemas.push(registered.schema.clone());
        }

        Ok(schemas)
    }

    fn tool_by_name(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools
            .iter()
            .find(|registered| registered.schema.name == name)
            .map(|registered| Arc::clone(&registered.tool))
    }
}

impl<P: Provider> Clone for Agent<P> {
    fn clone(&self) -> Self {
        Self {
            system_prompt: self.system_prompt.clone(),
            model: self.model.clone(),
            tools: self.tools.clone(),
        }
    }
}

impl<P: Provider> std::fmt::Debug for Agent<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Agent")
            .field("model_id", &self.model.id())
            .field("tool_count", &self.tools.len())
            .finish_non_exhaustive()
    }
}

fn validate_tool_name(name: &str) -> Result<()> {
    let valid = !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'));

    if valid {
        Ok(())
    } else {
        Err(Error::Config {
            message: format!("invalid tool name '{name}'"),
        })
    }
}

/// A conversation session with independent mutable history.
pub struct Session<P: Provider> {
    agent: Agent<P>,
    history: Vec<Message>,
}

impl<P: Provider> Session<P> {
    /// Sends a user turn and returns its event stream.
    pub fn send(&mut self, message: impl Into<UserMessage>) -> EventStream<'_> {
        let message = message.into();
        let tool_schemas = match self.agent.tool_schemas() {
            Ok(tool_schemas) => tool_schemas,
            Err(error) => return EventStream::from_error(error),
        };
        if let Err(error) = self.agent.model.validate_settings() {
            return EventStream::from_error(error);
        }

        let provider: Arc<dyn Provider> = self.agent.model.provider.clone();
        let tools = self
            .agent
            .tools
            .iter()
            .map(|registered| StreamTool {
                tool: Arc::clone(&registered.tool),
                schema: registered.schema.clone(),
            })
            .collect();

        EventStream::new(
            &mut self.history,
            provider,
            self.agent.model.id.clone(),
            tools,
            tool_schemas,
            self.agent.model.settings.clone(),
            message,
        )
    }

    /// Returns the accumulated complete conversation history.
    pub fn history(&self) -> &[Message] {
        &self.history
    }

    /// Drops every turn after the system prompt.
    pub fn reset(&mut self) {
        self.history.truncate(1);
    }
}

impl<P: Provider> Clone for Session<P> {
    fn clone(&self) -> Self {
        Self {
            agent: self.agent.clone(),
            history: self.history.clone(),
        }
    }
}

impl<P: Provider> std::fmt::Debug for Session<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("model_id", &self.agent.model.id())
            .field("history_len", &self.history.len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
fn poll_event_stream(
    stream: &mut EventStream<'_>,
) -> std::task::Poll<Option<Result<event::Event>>> {
    use std::pin::Pin;
    use std::task::Context;

    let mut cx = Context::from_waker(futures_util::task::noop_waker_ref());
    Pin::new(stream).poll_next(&mut cx)
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
    use crate::provider::{RawEvent, Request};
    use std::collections::VecDeque;
    use std::error::Error as StdError;
    use std::hash::Hash;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::task::Poll;
    use tracing::span::{Attributes, Record};
    use tracing::{Event as TracingEvent, Id, Level, Metadata, Subscriber};

    #[derive(Default)]
    struct FakeProviderState {
        spec_calls: AtomicUsize,
        notice_calls: AtomicUsize,
        chat_calls: AtomicUsize,
        requests: Mutex<Vec<Request>>,
        responses: Mutex<VecDeque<Vec<Result<RawEvent>>>>,
    }

    struct FakeProvider {
        state: Arc<FakeProviderState>,
    }

    impl FakeProvider {
        fn new(state: Arc<FakeProviderState>) -> Self {
            Self { state }
        }

        fn with_responses(
            state: Arc<FakeProviderState>,
            responses: impl IntoIterator<Item = Vec<Result<RawEvent>>>,
        ) -> Self {
            *state.responses.lock().unwrap() = responses.into_iter().collect();
            Self { state }
        }
    }

    impl Provider for FakeProvider {
        fn chat(&self, request: &Request) -> BoxStream<'static, Result<RawEvent>> {
            self.state.chat_calls.fetch_add(1, Ordering::Relaxed);
            self.state.requests.lock().unwrap().push(request.clone());
            let events = self
                .state
                .responses
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| vec![Ok(done_event())]);
            Box::pin(futures_util::stream::iter(events))
        }

        fn model_spec(&self, model_id: &str) -> Option<ModelSpec> {
            self.state.spec_calls.fetch_add(1, Ordering::Relaxed);
            (model_id == "known").then_some(ModelSpec::new(128, 16))
        }

        fn model_notice(&self, model_id: &str) -> Option<ModelNotice> {
            self.state.notice_calls.fetch_add(1, Ordering::Relaxed);
            (model_id == "deprecated").then_some(ModelNotice::Deprecated {
                message: "deprecated model".to_owned(),
            })
        }
    }

    struct TestTool {
        name: &'static str,
    }

    impl Tool for TestTool {
        fn name(&self) -> &str {
            self.name
        }

        fn schema(&self) -> ToolSchema {
            ToolSchema::new(self.name, serde_json::json!({ "type": "object" }))
        }

        fn call(
            &self,
            _args: tool::ToolArguments,
        ) -> BoxFuture<'static, std::result::Result<String, String>> {
            Box::pin(async { Ok("null".to_owned()) })
        }
    }

    struct RecordingTool {
        name: &'static str,
        output: std::result::Result<String, String>,
        calls: Arc<Mutex<Vec<String>>>,
    }

    impl Tool for RecordingTool {
        fn name(&self) -> &str {
            self.name
        }

        fn schema(&self) -> ToolSchema {
            ToolSchema::new(self.name, serde_json::json!({ "type": "object" }))
        }

        fn call(
            &self,
            args: tool::ToolArguments,
        ) -> BoxFuture<'static, std::result::Result<String, String>> {
            let calls = Arc::clone(&self.calls);
            let name = self.name.to_owned();
            let output = self.output.clone();
            Box::pin(async move {
                calls.lock().unwrap().push(format!("{name}:{args}"));
                output
            })
        }
    }

    struct ParsingTool {
        calls: Arc<Mutex<Vec<u32>>>,
    }

    impl Tool for ParsingTool {
        fn name(&self) -> &str {
            "parse"
        }

        fn schema(&self) -> ToolSchema {
            ToolSchema::new("parse", serde_json::json!({ "type": "object" }))
        }

        fn call(
            &self,
            args: tool::ToolArguments,
        ) -> BoxFuture<'static, std::result::Result<String, String>> {
            let calls = Arc::clone(&self.calls);
            Box::pin(async move {
                #[derive(serde::Deserialize)]
                struct Args {
                    n: u32,
                }

                let args = args.parse::<Args>().map_err(|error| error.to_string())?;
                calls.lock().unwrap().push(args.n);
                Ok(serde_json::to_string(&args.n).expect("u32 serializes"))
            })
        }
    }

    struct WarningCounter {
        warnings: Arc<AtomicUsize>,
    }

    impl Subscriber for WarningCounter {
        fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
            true
        }

        fn new_span(&self, _span: &Attributes<'_>) -> Id {
            Id::from_u64(1)
        }

        fn record(&self, _span: &Id, _values: &Record<'_>) {}

        fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

        fn event(&self, event: &TracingEvent<'_>) {
            if *event.metadata().level() == Level::WARN {
                self.warnings.fetch_add(1, Ordering::Relaxed);
            }
        }

        fn enter(&self, _span: &Id) {}

        fn exit(&self, _span: &Id) {}
    }

    fn next_event(stream: &mut EventStream<'_>) -> Option<Result<event::Event>> {
        match poll_event_stream(stream) {
            Poll::Ready(event) => event,
            Poll::Pending => panic!("test event stream should not pend"),
        }
    }

    fn done_event() -> RawEvent {
        RawEvent::Done {
            finish_reason: event::FinishReason::Stop,
            usage: None,
        }
    }

    fn assert_done(stream: &mut EventStream<'_>) {
        match next_event(stream) {
            Some(Ok(event::Event::Done {
                finish_reason: event::FinishReason::Stop,
            })) => {}
            other => panic!("expected terminal stop event, got {other:?}"),
        }
        assert!(next_event(stream).is_none());
    }

    fn ok_response(events: impl IntoIterator<Item = RawEvent>) -> Vec<Result<RawEvent>> {
        events.into_iter().map(Ok).collect()
    }

    fn usage(total_tokens: u32) -> event::Usage {
        event::Usage {
            prompt_tokens: total_tokens / 2,
            completion_tokens: total_tokens / 2,
            total_tokens,
            cached_prompt_tokens: None,
            reasoning_tokens: None,
        }
    }

    fn drain_ok(stream: &mut EventStream<'_>) -> Vec<event::Event> {
        let mut events = Vec::new();
        while let Some(event) = next_event(stream) {
            events.push(event.expect("stream should yield only ok events"));
        }
        events
    }

    fn assert_config_error(stream: &mut EventStream<'_>, expected: &str) {
        match next_event(stream) {
            Some(Err(Error::Config { message })) => {
                assert!(
                    message.contains(expected),
                    "expected config message containing {expected:?}, got {message:?}"
                );
            }
            other => panic!("expected config error, got {other:?}"),
        }
        assert!(next_event(stream).is_none());
    }

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

    #[test]
    fn model_exposes_cached_catalog_facts() {
        let known = Model::new(
            FakeProvider::new(Arc::new(FakeProviderState::default())),
            "known",
        );
        assert_eq!(known.id(), "known");
        assert_eq!(known.context_window(), Some(128));
        assert_eq!(known.max_output(), Some(16));

        let unknown = Model::new(
            FakeProvider::new(Arc::new(FakeProviderState::default())),
            "unknown",
        );
        assert_eq!(unknown.id(), "unknown");
        assert_eq!(unknown.context_window(), None);
        assert_eq!(unknown.max_output(), None);
    }

    #[test]
    fn model_construction_performs_catalog_and_notice_lookup_once() {
        let state = Arc::new(FakeProviderState::default());
        let model = Model::new(FakeProvider::new(Arc::clone(&state)), "known")
            .thinking(model::Thinking::Enabled)
            .max_tokens(8)
            .temperature(0.5);

        assert_eq!(state.spec_calls.load(Ordering::Relaxed), 1);
        assert_eq!(state.notice_calls.load(Ordering::Relaxed), 1);

        assert_eq!(model.context_window(), Some(128));
        assert_eq!(model.max_output(), Some(16));

        let agent = Agent::new("system", model);
        let mut session = agent.session();
        let mut stream = session.send("hello");

        assert_done(&mut stream);
        assert_eq!(state.chat_calls.load(Ordering::Relaxed), 1);
        assert_eq!(state.spec_calls.load(Ordering::Relaxed), 1);
        assert_eq!(state.notice_calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn deprecated_model_notice_emits_one_warning_per_construction() {
        let warnings = Arc::new(AtomicUsize::new(0));
        let subscriber = WarningCounter {
            warnings: Arc::clone(&warnings),
        };
        let dispatch = tracing::Dispatch::new(subscriber);

        tracing::dispatcher::with_default(&dispatch, || {
            let _ = Model::new(
                FakeProvider::new(Arc::new(FakeProviderState::default())),
                "deprecated",
            );
            assert_eq!(warnings.load(Ordering::Relaxed), 1);

            let _ = Model::new(
                FakeProvider::new(Arc::new(FakeProviderState::default())),
                "known",
            );
            assert_eq!(warnings.load(Ordering::Relaxed), 1);

            let _ = Model::new(
                FakeProvider::new(Arc::new(FakeProviderState::default())),
                "deprecated",
            );
            assert_eq!(warnings.load(Ordering::Relaxed), 2);
        });
    }

    #[test]
    fn max_tokens_validation_errors_before_provider_chat() {
        let zero_state = Arc::new(FakeProviderState::default());
        let zero_model =
            Model::new(FakeProvider::new(Arc::clone(&zero_state)), "known").max_tokens(0);
        let mut zero_session = Agent::new("system", zero_model).session();
        let mut zero_stream = zero_session.send("hello");
        assert_config_error(&mut zero_stream, "at least 1");
        assert_eq!(zero_state.chat_calls.load(Ordering::Relaxed), 0);

        let over_state = Arc::new(FakeProviderState::default());
        let over_model =
            Model::new(FakeProvider::new(Arc::clone(&over_state)), "known").max_tokens(17);
        let mut over_session = Agent::new("system", over_model).session();
        let mut over_stream = over_session.send("hello");
        assert_config_error(&mut over_stream, "exceeds");
        assert_eq!(over_state.chat_calls.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn unknown_model_has_no_local_max_tokens_upper_cap() {
        let state = Arc::new(FakeProviderState::default());
        let model = Model::new(FakeProvider::new(Arc::clone(&state)), "unknown").max_tokens(999);
        let mut session = Agent::new("system", model).session();
        let mut stream = session.send("hello");

        assert_done(&mut stream);
        assert_eq!(state.chat_calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn request_construction_carries_settings_verbatim() {
        let state = Arc::new(FakeProviderState::default());
        let model = Model::new(FakeProvider::new(Arc::clone(&state)), "known")
            .thinking(model::Thinking::Disabled)
            .reasoning_effort(model::ReasoningEffort::High)
            .max_tokens(8)
            .temperature(0.7)
            .top_p(0.9)
            .stop(vec!["END".to_owned(), "STOP".to_owned()])
            .response_format(model::ResponseFormat::JsonObject);
        let mut session = Agent::new("system", model).session();
        let mut stream = session.send("hello");

        assert_done(&mut stream);

        let requests = state.requests.lock().unwrap();
        let request = requests.last().unwrap();
        assert_eq!(request.model, "known");
        assert_eq!(request.messages.len(), 2);
        assert_eq!(request.messages[0].role(), provider::MessageRole::System);
        assert_eq!(request.messages[0].content(), Some("system"));
        assert_eq!(request.messages[1].role(), provider::MessageRole::User);
        assert_eq!(request.messages[1].content(), Some("hello"));
        assert_eq!(request.settings.thinking, model::Thinking::Disabled);
        assert_eq!(
            request.settings.reasoning_effort,
            Some(model::ReasoningEffort::High)
        );
        assert_eq!(request.settings.max_tokens, Some(8));
        assert_eq!(request.settings.temperature, Some(0.7));
        assert_eq!(request.settings.top_p, Some(0.9));
        assert_eq!(request.settings.stop, ["END", "STOP"]);
        assert_eq!(
            request.settings.response_format,
            model::ResponseFormat::JsonObject
        );
    }

    #[test]
    fn sessions_from_one_agent_have_independent_history() {
        let model = Model::new(
            FakeProvider::new(Arc::new(FakeProviderState::default())),
            "known",
        );
        let agent = Agent::new("system", model);
        let mut first = agent.session();
        let second = agent.session();

        first.history.push(Message::user("first only"));

        assert_eq!(first.history().len(), 2);
        assert_eq!(second.history().len(), 1);
        assert_eq!(second.history()[0].content(), Some("system"));
    }

    #[test]
    fn session_reset_keeps_system_prompt_and_session_reusable() {
        let state = Arc::new(FakeProviderState::default());
        let model = Model::new(FakeProvider::new(Arc::clone(&state)), "known");
        let mut session = Agent::new("system", model).session();

        session.history.push(Message::user("old"));
        session.history.push(Message::assistant(
            Some("answer".to_owned()),
            Some("reasoning".to_owned()),
            Vec::new(),
        ));
        session.reset();

        assert_eq!(session.history().len(), 1);
        assert_eq!(session.history()[0].role(), provider::MessageRole::System);
        assert_eq!(session.history()[0].content(), Some("system"));

        let mut stream = session.send("new");
        assert_done(&mut stream);

        let requests = state.requests.lock().unwrap();
        let request = requests.last().unwrap();
        assert_eq!(request.messages.len(), 2);
        assert_eq!(request.messages[1].content(), Some("new"));
    }

    #[test]
    fn tool_schemas_are_requested_in_registration_order() {
        let state = Arc::new(FakeProviderState::default());
        let model = Model::new(FakeProvider::new(Arc::clone(&state)), "known");
        let agent = Agent::new("system", model)
            .tool(TestTool { name: "first" })
            .tool(TestTool { name: "second" });
        let mut session = agent.session();
        let mut stream = session.send("hello");

        assert_done(&mut stream);

        let requests = state.requests.lock().unwrap();
        let request = requests.last().unwrap();
        let names = request
            .tools
            .iter()
            .map(|schema| schema.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, ["first", "second"]);
    }

    #[test]
    fn registered_tools_can_be_looked_up_by_name() {
        let model = Model::new(
            FakeProvider::new(Arc::new(FakeProviderState::default())),
            "known",
        );
        let agent = Agent::new("system", model)
            .tool(TestTool { name: "first" })
            .tool(TestTool { name: "second" });

        assert_eq!(agent.tool_by_name("first").unwrap().name(), "first");
        assert_eq!(agent.tool_by_name("second").unwrap().name(), "second");
        assert!(agent.tool_by_name("missing").is_none());
    }

    #[test]
    fn duplicate_or_invalid_tool_names_error_before_provider_chat() {
        let duplicate_state = Arc::new(FakeProviderState::default());
        let duplicate_model = Model::new(FakeProvider::new(Arc::clone(&duplicate_state)), "known");
        let duplicate_agent = Agent::new("system", duplicate_model)
            .tool(TestTool { name: "same" })
            .tool(TestTool { name: "same" });
        let mut duplicate_session = duplicate_agent.session();
        let mut duplicate_stream = duplicate_session.send("hello");
        assert_config_error(&mut duplicate_stream, "duplicate tool name");
        assert_eq!(duplicate_state.chat_calls.load(Ordering::Relaxed), 0);

        let invalid_state = Arc::new(FakeProviderState::default());
        let invalid_model = Model::new(FakeProvider::new(Arc::clone(&invalid_state)), "known");
        let invalid_agent =
            Agent::new("system", invalid_model).tool(TestTool { name: "not valid" });
        let mut invalid_session = invalid_agent.session();
        let mut invalid_stream = invalid_session.send("hello");
        assert_config_error(&mut invalid_stream, "invalid tool name");
        assert_eq!(invalid_state.chat_calls.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn tool_round_events_are_ordered_and_done_is_terminal() {
        let state = Arc::new(FakeProviderState::default());
        let model = Model::new(
            FakeProvider::with_responses(
                Arc::clone(&state),
                [
                    ok_response([
                        RawEvent::TextDelta("calling ".to_owned()),
                        RawEvent::ReasoningDelta("need tool".to_owned()),
                        RawEvent::ToolCallDelta {
                            index: 0,
                            id: Some("call-1".to_owned()),
                            name: Some("add".to_owned()),
                            arguments: r#"{"a":"#.to_owned(),
                        },
                        RawEvent::ToolCallDelta {
                            index: 0,
                            id: None,
                            name: None,
                            arguments: "1}".to_owned(),
                        },
                        RawEvent::Done {
                            finish_reason: event::FinishReason::ToolCalls,
                            usage: Some(usage(10)),
                        },
                    ]),
                    ok_response([
                        RawEvent::TextDelta("done".to_owned()),
                        RawEvent::Done {
                            finish_reason: event::FinishReason::Stop,
                            usage: Some(usage(12)),
                        },
                    ]),
                ],
            ),
            "known",
        );
        let calls = Arc::new(Mutex::new(Vec::new()));
        let agent = Agent::new("system", model).tool(RecordingTool {
            name: "add",
            output: Ok("1".to_owned()),
            calls: Arc::clone(&calls),
        });
        let mut session = agent.session();

        let mut stream = session.send("use a tool");
        let events = drain_ok(&mut stream);
        drop(stream);

        assert_eq!(
            events,
            vec![
                event::Event::TextDelta {
                    delta: "calling ".to_owned()
                },
                event::Event::ReasoningDelta {
                    delta: "need tool".to_owned()
                },
                event::Event::Usage { usage: usage(10) },
                event::Event::ToolCall {
                    id: "call-1".to_owned(),
                    name: "add".to_owned(),
                    arguments: r#"{"a":1}"#.into(),
                },
                event::Event::ToolResult {
                    id: "call-1".to_owned(),
                    name: "add".to_owned(),
                    output: event::ToolOutput::Ok("1".to_owned()),
                },
                event::Event::TextDelta {
                    delta: "done".to_owned()
                },
                event::Event::Usage { usage: usage(12) },
                event::Event::Done {
                    finish_reason: event::FinishReason::Stop,
                },
            ]
        );
        assert_eq!(*calls.lock().unwrap(), vec![r#"add:{"a":1}"#]);

        let requests = state.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[1].messages.len(), 4);
        assert_eq!(
            requests[1].messages[2].role(),
            provider::MessageRole::Assistant
        );
        assert_eq!(requests[1].messages[2].content(), Some("calling "));
        assert_eq!(
            requests[1].messages[2].reasoning_content(),
            Some("need tool")
        );
        assert_eq!(requests[1].messages[2].tool_calls().len(), 1);
        assert_eq!(requests[1].messages[3].role(), provider::MessageRole::Tool);
        assert_eq!(requests[1].messages[3].tool_call_id(), Some("call-1"));
        assert_eq!(requests[1].messages[3].content(), Some("1"));

        assert_eq!(session.history().len(), 5);
        assert_eq!(session.history()[2].reasoning_content(), Some("need tool"));
        assert_eq!(session.history()[3].tool_call_id(), Some("call-1"));
        assert_eq!(session.history()[4].content(), Some("done"));
    }

    #[test]
    fn multiple_tool_calls_run_sequentially_by_index() {
        let state = Arc::new(FakeProviderState::default());
        let model = Model::new(
            FakeProvider::with_responses(
                Arc::clone(&state),
                [
                    ok_response([
                        RawEvent::ToolCallDelta {
                            index: 1,
                            id: Some("call-2".to_owned()),
                            name: Some("second".to_owned()),
                            arguments: "{}".to_owned(),
                        },
                        RawEvent::ToolCallDelta {
                            index: 0,
                            id: Some("call-1".to_owned()),
                            name: Some("first".to_owned()),
                            arguments: r#"{"x":"#.to_owned(),
                        },
                        RawEvent::ToolCallDelta {
                            index: 0,
                            id: None,
                            name: None,
                            arguments: "1}".to_owned(),
                        },
                        RawEvent::Done {
                            finish_reason: event::FinishReason::ToolCalls,
                            usage: None,
                        },
                    ]),
                    ok_response([done_event()]),
                ],
            ),
            "known",
        );
        let calls = Arc::new(Mutex::new(Vec::new()));
        let agent = Agent::new("system", model)
            .tool(RecordingTool {
                name: "first",
                output: Ok(r#""one""#.to_owned()),
                calls: Arc::clone(&calls),
            })
            .tool(RecordingTool {
                name: "second",
                output: Ok(r#""two""#.to_owned()),
                calls: Arc::clone(&calls),
            });
        let mut session = agent.session();

        let mut stream = session.send("run both");
        let events = drain_ok(&mut stream);
        drop(stream);

        let tool_events = events
            .iter()
            .filter_map(|event| match event {
                event::Event::ToolCall { id, .. } => Some(format!("call:{id}")),
                event::Event::ToolResult { id, output, .. } => {
                    Some(format!("result:{id}:{}", output.content()))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            tool_events,
            [
                "call:call-1",
                "result:call-1:\"one\"",
                "call:call-2",
                "result:call-2:\"two\""
            ]
        );
        assert_eq!(
            *calls.lock().unwrap(),
            vec![r#"first:{"x":1}"#, "second:{}"]
        );
        assert_eq!(session.history()[3].tool_call_id(), Some("call-1"));
        assert_eq!(session.history()[3].content(), Some(r#""one""#));
        assert_eq!(session.history()[4].tool_call_id(), Some("call-2"));
        assert_eq!(session.history()[4].content(), Some(r#""two""#));
    }

    #[test]
    fn unknown_tool_appends_error_result_and_continues() {
        let state = Arc::new(FakeProviderState::default());
        let model = Model::new(
            FakeProvider::with_responses(
                Arc::clone(&state),
                [
                    ok_response([
                        RawEvent::ToolCallDelta {
                            index: 0,
                            id: Some("missing-1".to_owned()),
                            name: Some("missing".to_owned()),
                            arguments: "{}".to_owned(),
                        },
                        RawEvent::Done {
                            finish_reason: event::FinishReason::ToolCalls,
                            usage: None,
                        },
                    ]),
                    ok_response([RawEvent::TextDelta("retried".to_owned()), done_event()]),
                ],
            ),
            "known",
        );
        let mut session = Agent::new("system", model).session();

        let mut stream = session.send("use missing tool");
        let events = drain_ok(&mut stream);
        drop(stream);

        let output = events
            .iter()
            .find_map(|event| match event {
                event::Event::ToolResult { output, .. } => Some(output),
                _ => None,
            })
            .expect("tool result is emitted");
        assert!(matches!(output, event::ToolOutput::Err(_)));
        assert!(output.content().contains("unknown tool 'missing'"));
        assert_eq!(session.history()[3].content(), Some(output.content()));
        assert_eq!(session.history()[4].content(), Some("retried"));
    }

    #[test]
    fn malformed_tool_arguments_are_tool_errors_not_turn_errors() {
        let state = Arc::new(FakeProviderState::default());
        let model = Model::new(
            FakeProvider::with_responses(
                Arc::clone(&state),
                [
                    ok_response([
                        RawEvent::ToolCallDelta {
                            index: 0,
                            id: Some("parse-1".to_owned()),
                            name: Some("parse".to_owned()),
                            arguments: "not json".to_owned(),
                        },
                        RawEvent::Done {
                            finish_reason: event::FinishReason::ToolCalls,
                            usage: None,
                        },
                    ]),
                    ok_response([RawEvent::TextDelta("retry ok".to_owned()), done_event()]),
                ],
            ),
            "known",
        );
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut session = Agent::new("system", model)
            .tool(ParsingTool {
                calls: Arc::clone(&calls),
            })
            .session();

        let mut stream = session.send("parse this");
        let events = drain_ok(&mut stream);
        drop(stream);

        let output = events
            .iter()
            .find_map(|event| match event {
                event::Event::ToolResult { output, .. } => Some(output),
                _ => None,
            })
            .expect("tool result is emitted");
        assert!(matches!(output, event::ToolOutput::Err(_)));
        assert!(calls.lock().unwrap().is_empty());
        assert_eq!(session.history()[3].tool_call_id(), Some("parse-1"));
        assert_eq!(session.history()[3].content(), Some(output.content()));
        assert_eq!(session.history()[4].content(), Some("retry ok"));
    }

    #[test]
    fn provider_error_rolls_back_pending_turn() {
        let state = Arc::new(FakeProviderState::default());
        let model = Model::new(
            FakeProvider::with_responses(
                Arc::clone(&state),
                [vec![
                    Ok(RawEvent::TextDelta("partial".to_owned())),
                    Err(Error::Server {
                        status: 500,
                        message: "down".to_owned(),
                    }),
                ]],
            ),
            "known",
        );
        let mut session = Agent::new("system", model).session();
        let before = session.history().to_vec();

        let mut stream = session.send("hello");
        assert_eq!(
            next_event(&mut stream).unwrap().unwrap(),
            event::Event::TextDelta {
                delta: "partial".to_owned()
            }
        );
        match next_event(&mut stream) {
            Some(Err(Error::Server { status: 500, .. })) => {}
            other => panic!("expected provider error, got {other:?}"),
        }
        drop(stream);

        assert_eq!(session.history(), before.as_slice());
    }

    #[test]
    fn tool_calls_finish_with_no_calls_errors_and_rolls_back() {
        let state = Arc::new(FakeProviderState::default());
        let model = Model::new(
            FakeProvider::with_responses(
                Arc::clone(&state),
                [ok_response([RawEvent::Done {
                    finish_reason: event::FinishReason::ToolCalls,
                    usage: None,
                }])],
            ),
            "known",
        );
        let mut session = Agent::new("system", model).session();
        let before = session.history().to_vec();

        let mut stream = session.send("bad provider");
        match next_event(&mut stream) {
            Some(Err(Error::Decode { context, source })) => {
                assert_eq!(context, "assembling tool calls");
                assert!(source.to_string().contains("emitted no tool calls"));
            }
            other => panic!("expected decode error, got {other:?}"),
        }
        drop(stream);

        assert_eq!(session.history(), before.as_slice());
        assert_eq!(state.chat_calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn provider_error_after_tool_result_rolls_back_whole_turn() {
        let state = Arc::new(FakeProviderState::default());
        let model = Model::new(
            FakeProvider::with_responses(
                Arc::clone(&state),
                [
                    ok_response([
                        RawEvent::ToolCallDelta {
                            index: 0,
                            id: Some("call-1".to_owned()),
                            name: Some("add".to_owned()),
                            arguments: "{}".to_owned(),
                        },
                        RawEvent::Done {
                            finish_reason: event::FinishReason::ToolCalls,
                            usage: None,
                        },
                    ]),
                    vec![
                        Ok(RawEvent::TextDelta("partial retry".to_owned())),
                        Err(Error::Server {
                            status: 502,
                            message: "retry failed".to_owned(),
                        }),
                    ],
                ],
            ),
            "known",
        );
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut session = Agent::new("system", model)
            .tool(RecordingTool {
                name: "add",
                output: Ok("1".to_owned()),
                calls,
            })
            .session();
        let before = session.history().to_vec();

        let mut stream = session.send("tool then fail");
        assert!(matches!(
            next_event(&mut stream),
            Some(Ok(event::Event::ToolCall { .. }))
        ));
        assert!(matches!(
            next_event(&mut stream),
            Some(Ok(event::Event::ToolResult { .. }))
        ));
        assert_eq!(
            next_event(&mut stream).unwrap().unwrap(),
            event::Event::TextDelta {
                delta: "partial retry".to_owned()
            }
        );
        match next_event(&mut stream) {
            Some(Err(Error::Server { status: 502, .. })) => {}
            other => panic!("expected provider error, got {other:?}"),
        }
        drop(stream);

        assert_eq!(session.history(), before.as_slice());
    }

    #[test]
    fn dropping_stream_before_done_rolls_back_pending_turn() {
        let state = Arc::new(FakeProviderState::default());
        let model = Model::new(
            FakeProvider::with_responses(
                Arc::clone(&state),
                [ok_response([
                    RawEvent::TextDelta("partial".to_owned()),
                    done_event(),
                ])],
            ),
            "known",
        );
        let mut session = Agent::new("system", model).session();
        let before = session.history().to_vec();

        let mut stream = session.send("hello");
        assert_eq!(
            next_event(&mut stream).unwrap().unwrap(),
            event::Event::TextDelta {
                delta: "partial".to_owned()
            }
        );
        drop(stream);

        assert_eq!(session.history(), before.as_slice());
    }

    #[test]
    fn dropping_stream_after_tool_result_rolls_back_whole_turn() {
        let state = Arc::new(FakeProviderState::default());
        let model = Model::new(
            FakeProvider::with_responses(
                Arc::clone(&state),
                [
                    ok_response([
                        RawEvent::ToolCallDelta {
                            index: 0,
                            id: Some("call-1".to_owned()),
                            name: Some("add".to_owned()),
                            arguments: "{}".to_owned(),
                        },
                        RawEvent::Done {
                            finish_reason: event::FinishReason::ToolCalls,
                            usage: None,
                        },
                    ]),
                    ok_response([
                        RawEvent::TextDelta("partial retry".to_owned()),
                        done_event(),
                    ]),
                ],
            ),
            "known",
        );
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut session = Agent::new("system", model)
            .tool(RecordingTool {
                name: "add",
                output: Ok("1".to_owned()),
                calls,
            })
            .session();
        let before = session.history().to_vec();

        let mut stream = session.send("tool then drop");
        assert!(matches!(
            next_event(&mut stream),
            Some(Ok(event::Event::ToolCall { .. }))
        ));
        assert!(matches!(
            next_event(&mut stream),
            Some(Ok(event::Event::ToolResult { .. }))
        ));
        assert_eq!(
            next_event(&mut stream).unwrap().unwrap(),
            event::Event::TextDelta {
                delta: "partial retry".to_owned()
            }
        );
        drop(stream);

        assert_eq!(session.history(), before.as_slice());
    }

    #[test]
    fn complete_turn_commits_user_assistant_and_reasoning_atomically() {
        let state = Arc::new(FakeProviderState::default());
        let model = Model::new(
            FakeProvider::with_responses(
                Arc::clone(&state),
                [ok_response([
                    RawEvent::ReasoningDelta("because".to_owned()),
                    RawEvent::TextDelta("answer".to_owned()),
                    done_event(),
                ])],
            ),
            "known",
        );
        let mut session = Agent::new("system", model).session();

        let mut stream = session.send("hello");
        assert_eq!(
            drain_ok(&mut stream),
            vec![
                event::Event::ReasoningDelta {
                    delta: "because".to_owned()
                },
                event::Event::TextDelta {
                    delta: "answer".to_owned()
                },
                event::Event::Done {
                    finish_reason: event::FinishReason::Stop
                }
            ]
        );
        drop(stream);

        assert_eq!(session.history().len(), 3);
        assert_eq!(session.history()[1].role(), provider::MessageRole::User);
        assert_eq!(session.history()[1].content(), Some("hello"));
        assert_eq!(
            session.history()[2].role(),
            provider::MessageRole::Assistant
        );
        assert_eq!(session.history()[2].content(), Some("answer"));
        assert_eq!(session.history()[2].reasoning_content(), Some("because"));
    }

    #[test]
    fn handle_debug_impls_do_not_require_provider_debug() {
        let model = Model::new(
            FakeProvider::new(Arc::new(FakeProviderState::default())),
            "known",
        );
        assert!(format!("{model:?}").contains("Model"));
        let agent = Agent::new("system", model);
        assert!(format!("{agent:?}").contains("Agent"));
        let session = agent.session();

        assert!(format!("{session:?}").contains("Session"));
        assert!(format!("{:?}", EventStream::empty()).contains("EventStream"));
    }
}
