//! Event types produced by the agent loop.

use std::collections::{BTreeMap, VecDeque};
use std::hash::Hash;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use crate::provider::{Message, Provider, RawEvent, Request, Settings, ToolCall};
use crate::tool::{Tool, ToolArguments, ToolSchema};
use crate::{BoxFuture, BoxStream, Error, Result, Stream, UserMessage};

/// Opaque stream of events returned by a session.
pub struct EventStream<'a> {
    session_history: Option<&'a mut Vec<Message>>,
    pending_history: Vec<Message>,
    provider: Option<Arc<dyn Provider>>,
    model: String,
    tools: Vec<StreamTool>,
    tool_schemas: Vec<ToolSchema>,
    settings: Settings,
    provider_stream: Option<BoxStream<'static, Result<RawEvent>>>,
    assistant: AssistantTurn,
    ready: VecDeque<QueuedEvent>,
    continue_after_tools: bool,
    tool_calls: VecDeque<ToolCall>,
    tool_to_start: Option<ToolCall>,
    running_tool: Option<RunningTool>,
    finished: bool,
}

impl<'a> EventStream<'a> {
    pub(crate) fn new(
        session_history: &'a mut Vec<Message>,
        provider: Arc<dyn Provider>,
        model: String,
        tools: Vec<StreamTool>,
        tool_schemas: Vec<ToolSchema>,
        settings: Settings,
        message: UserMessage,
    ) -> Self {
        let mut pending_history = session_history.clone();
        pending_history.push(Message::user(message.as_str()));

        let mut stream = Self {
            session_history: Some(session_history),
            pending_history,
            provider: Some(provider),
            model,
            tools,
            tool_schemas,
            settings,
            provider_stream: None,
            assistant: AssistantTurn::default(),
            ready: VecDeque::new(),
            continue_after_tools: false,
            tool_calls: VecDeque::new(),
            tool_to_start: None,
            running_tool: None,
            finished: false,
        };
        stream.start_provider_turn();
        stream
    }

    #[cfg(test)]
    pub(crate) fn empty() -> Self {
        Self {
            session_history: None,
            pending_history: Vec::new(),
            provider: None,
            model: String::new(),
            tools: Vec::new(),
            tool_schemas: Vec::new(),
            settings: Settings::default(),
            provider_stream: None,
            assistant: AssistantTurn::default(),
            ready: VecDeque::new(),
            continue_after_tools: false,
            tool_calls: VecDeque::new(),
            tool_to_start: None,
            running_tool: None,
            finished: true,
        }
    }

    pub(crate) fn from_error(error: crate::Error) -> Self {
        Self {
            session_history: None,
            pending_history: Vec::new(),
            provider: None,
            model: String::new(),
            tools: Vec::new(),
            tool_schemas: Vec::new(),
            settings: Settings::default(),
            provider_stream: None,
            assistant: AssistantTurn::default(),
            ready: VecDeque::from([QueuedEvent::Error(error)]),
            continue_after_tools: false,
            tool_calls: VecDeque::new(),
            tool_to_start: None,
            running_tool: None,
            finished: false,
        }
    }

    fn start_provider_turn(&mut self) {
        let Some(provider) = &self.provider else {
            self.finished = true;
            return;
        };

        let request = Request {
            model: self.model.clone(),
            messages: self.pending_history.clone(),
            tools: self.tool_schemas.clone(),
            settings: self.settings.clone(),
        };
        self.assistant = AssistantTurn::default();
        self.provider_stream = Some(provider.chat(&request));
    }

    fn handle_raw_event(&mut self, event: RawEvent) {
        match event {
            RawEvent::TextDelta(delta) => {
                self.assistant.content.push_str(&delta);
                self.ready
                    .push_back(QueuedEvent::Event(Event::TextDelta { delta }));
            }
            RawEvent::ReasoningDelta(delta) => {
                self.assistant.reasoning_content.push_str(&delta);
                self.ready
                    .push_back(QueuedEvent::Event(Event::ReasoningDelta { delta }));
            }
            RawEvent::ToolCallDelta {
                index,
                id,
                name,
                arguments,
            } => self
                .assistant
                .push_tool_call_fragment(index, id, name, arguments),
            RawEvent::Done {
                finish_reason,
                usage,
            } => {
                if let Some(usage) = usage {
                    self.ready
                        .push_back(QueuedEvent::Event(Event::Usage { usage }));
                }

                let is_tool_call_turn = finish_reason == FinishReason::ToolCalls;
                let assistant = std::mem::take(&mut self.assistant);
                let tool_calls = match assistant.tool_calls() {
                    Ok(tool_calls) => tool_calls,
                    Err(error) => {
                        self.fail(error);
                        return;
                    }
                };

                self.pending_history.push(Message::assistant(
                    non_empty(assistant.content),
                    non_empty(assistant.reasoning_content),
                    tool_calls.clone(),
                ));
                self.provider_stream = None;

                if is_tool_call_turn {
                    if tool_calls.is_empty() {
                        self.fail(missing_tool_calls());
                        return;
                    }

                    self.continue_after_tools = true;
                    self.tool_calls = tool_calls.into();
                } else {
                    self.ready.push_back(QueuedEvent::Done(finish_reason));
                }
            }
        }
    }

    fn queue_next_tool_call(&mut self) {
        let call = self
            .tool_calls
            .pop_front()
            .expect("tool call queued while continuing after tools");
        self.tool_to_start = Some(call.clone());
        self.ready.push_back(QueuedEvent::Event(Event::ToolCall {
            id: call.id,
            name: call.name,
            arguments: call.arguments,
        }));
    }

    fn start_tool(&mut self, call: ToolCall) {
        let Some(tool) = self
            .tools
            .iter()
            .find(|tool| tool.name == call.name)
            .map(|tool| Arc::clone(&tool.tool))
        else {
            let output = ToolOutput::Err(format!("unknown tool '{}'", call.name));
            self.finish_tool(call, output);
            return;
        };

        let arguments = call.arguments.clone();
        self.running_tool = Some(RunningTool {
            call,
            future: tool.call(arguments),
        });
    }

    fn finish_tool(&mut self, call: ToolCall, output: ToolOutput) {
        self.pending_history
            .push(Message::tool(call.id.clone(), output.content()));
        self.ready.push_back(QueuedEvent::Event(Event::ToolResult {
            id: call.id,
            name: call.name,
            output,
        }));
    }

    fn fail(&mut self, error: Error) {
        self.provider_stream = None;
        self.running_tool = None;
        self.tool_to_start = None;
        self.continue_after_tools = false;
        self.ready.push_back(QueuedEvent::Error(error));
    }

    fn commit(&mut self) {
        if let Some(history) = self.session_history.as_mut() {
            **history = std::mem::take(&mut self.pending_history);
        }
        self.session_history = None;
    }
}

impl std::fmt::Debug for EventStream<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventStream").finish_non_exhaustive()
    }
}

impl Unpin for EventStream<'_> {}

impl Stream for EventStream<'_> {
    type Item = Result<Event>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        loop {
            if let Some(event) = this.ready.pop_front() {
                return match event {
                    QueuedEvent::Event(event) => Poll::Ready(Some(Ok(event))),
                    QueuedEvent::Error(error) => {
                        this.finished = true;
                        Poll::Ready(Some(Err(error)))
                    }
                    QueuedEvent::Done(finish_reason) => {
                        this.commit();
                        this.finished = true;
                        Poll::Ready(Some(Ok(Event::Done { finish_reason })))
                    }
                };
            }

            if this.finished {
                return Poll::Ready(None);
            }

            if let Some(call) = this.tool_to_start.take() {
                this.start_tool(call);
                continue;
            }

            if let Some(running_tool) = this.running_tool.as_mut() {
                match running_tool.future.as_mut().poll(cx) {
                    Poll::Ready(output) => {
                        let running_tool = this
                            .running_tool
                            .take()
                            .expect("running tool exists while it is being polled");
                        this.finish_tool(running_tool.call, ToolOutput::from_result(output));
                        continue;
                    }
                    Poll::Pending => return Poll::Pending,
                }
            }

            if this.continue_after_tools {
                if !this.tool_calls.is_empty() {
                    this.queue_next_tool_call();
                    continue;
                }

                this.continue_after_tools = false;
                this.start_provider_turn();
                continue;
            }

            let Some(provider_stream) = this.provider_stream.as_mut() else {
                this.finished = true;
                return Poll::Ready(None);
            };

            match provider_stream.as_mut().poll_next(cx) {
                Poll::Ready(Some(Ok(event))) => {
                    this.handle_raw_event(event);
                    continue;
                }
                Poll::Ready(Some(Err(error))) => {
                    this.fail(error);
                    continue;
                }
                Poll::Ready(None) => {
                    this.fail(unexpected_provider_eof());
                    continue;
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

pub(crate) struct StreamTool {
    pub(crate) tool: Arc<dyn Tool>,
    pub(crate) name: String,
}

struct RunningTool {
    call: ToolCall,
    future: BoxFuture<'static, std::result::Result<String, String>>,
}

enum QueuedEvent {
    Event(Event),
    Error(Error),
    Done(FinishReason),
}

#[derive(Default)]
struct AssistantTurn {
    content: String,
    reasoning_content: String,
    tool_calls: BTreeMap<u32, ToolCallBuilder>,
}

impl AssistantTurn {
    fn push_tool_call_fragment(
        &mut self,
        index: u32,
        id: Option<String>,
        name: Option<String>,
        arguments: String,
    ) {
        let call = self.tool_calls.entry(index).or_default();
        if call.id.is_none() {
            call.id = id;
        }
        if call.name.is_none() {
            call.name = name;
        }
        call.arguments.push_str(&arguments);
    }

    fn tool_calls(&self) -> Result<Vec<ToolCall>> {
        self.tool_calls
            .iter()
            .map(|(index, call)| {
                let id = call
                    .id
                    .clone()
                    .ok_or_else(|| missing_tool_field(*index, "id"))?;
                let name = call
                    .name
                    .clone()
                    .ok_or_else(|| missing_tool_field(*index, "name"))?;
                Ok(ToolCall::new(id, name, call.arguments.clone()))
            })
            .collect()
    }
}

#[derive(Default)]
struct ToolCallBuilder {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

fn non_empty(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

fn missing_tool_field(index: u32, field: &str) -> Error {
    Error::Decode {
        context: format!("assembling tool call {index}"),
        source: Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("missing tool call {field}"),
        )),
    }
}

fn missing_tool_calls() -> Error {
    Error::Decode {
        context: "assembling tool calls".to_owned(),
        source: Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "provider finished with tool_calls but emitted no tool calls",
        )),
    }
}

fn unexpected_provider_eof() -> Error {
    Error::Decode {
        context: "reading provider stream".to_owned(),
        source: Box::new(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "provider stream ended before RawEvent::Done",
        )),
    }
}

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
