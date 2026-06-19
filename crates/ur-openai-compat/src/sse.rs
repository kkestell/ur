//! SSE line framing, completion-state folding, and the wire response structs
//! shared by the OpenAI-compatible providers. Each provider supplies its own
//! `decode_chunk` (and its own `Delta` / usage types) over these primitives.

use std::collections::VecDeque;

use serde::Deserialize;
use ur_core::Error;
use ur_core::event::{FinishReason, Usage};
use ur_core::provider::RawEvent;

/// A normalized item produced by a provider's `decode_chunk` or by the `[DONE]`
/// sentinel.
#[derive(Debug, Eq, PartialEq)]
pub enum SseItem {
    Events(Vec<RawEvent>),
    Usage(Usage),
    Done,
}

/// One framed SSE payload: a data line to decode, or the `[DONE]` sentinel.
pub enum Frame {
    Data(String),
    Done,
}

/// A provider chunk decoder: parses one SSE data payload into normalized items.
pub type DecodeChunk = fn(&str) -> Result<Vec<SseItem>, Error>;

/// Incremental SSE line framer. Splits the byte stream into events on blank
/// lines, strips `data:` prefixes, ignores `:` comments, and recognizes the
/// `[DONE]` sentinel. Carries no provider semantics.
#[derive(Default)]
pub struct SseDecoder {
    buffer: Vec<u8>,
    data_lines: Vec<String>,
}

enum LineAction {
    Dispatch,
    Ignore,
    Data(String),
}

fn decode_line(line: &[u8]) -> Result<&str, Error> {
    std::str::from_utf8(line).map_err(|source| Error::Decode {
        context: "reading SSE line".to_owned(),
        source: Box::new(source),
    })
}

fn classify_line(line: &str) -> LineAction {
    if line.is_empty() {
        return LineAction::Dispatch;
    }
    if line.starts_with(':') {
        return LineAction::Ignore;
    }
    let Some(data) = line.strip_prefix("data:") else {
        return LineAction::Ignore;
    };
    LineAction::Data(data.strip_prefix(' ').unwrap_or(data).to_owned())
}

impl SseDecoder {
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<Frame>, Error> {
        self.buffer.extend_from_slice(bytes);
        self.drain_lines()
    }

    pub fn finish(&mut self) -> Result<Vec<Frame>, Error> {
        let mut frames = Vec::new();
        if !self.buffer.is_empty() {
            let mut line = std::mem::take(&mut self.buffer);
            if line.ends_with(b"\r") {
                line.pop();
            }
            let action = classify_line(decode_line(&line)?);
            frames.extend(self.apply_line(action));
        }

        if !self.data_lines.is_empty() {
            frames.extend(self.dispatch_event());
        }

        Ok(frames)
    }

    fn drain_lines(&mut self) -> Result<Vec<Frame>, Error> {
        let mut frames = Vec::new();
        while let Some(position) = self.buffer.iter().position(|byte| *byte == b'\n') {
            let mut end = position;
            if end > 0 && self.buffer[end - 1] == b'\r' {
                end -= 1;
            }
            let action = classify_line(decode_line(&self.buffer[..end])?);
            self.buffer.drain(..=position);
            frames.extend(self.apply_line(action));
        }
        Ok(frames)
    }

    fn apply_line(&mut self, action: LineAction) -> Vec<Frame> {
        match action {
            LineAction::Dispatch => self.dispatch_event(),
            LineAction::Ignore => Vec::new(),
            LineAction::Data(data) => {
                self.data_lines.push(data);
                Vec::new()
            }
        }
    }

    fn dispatch_event(&mut self) -> Vec<Frame> {
        if self.data_lines.is_empty() {
            return Vec::new();
        }

        let mut lines = std::mem::take(&mut self.data_lines);
        let data = if lines.len() == 1 {
            lines.pop().unwrap()
        } else {
            lines.join("\n")
        };
        if data == "[DONE]" {
            return vec![Frame::Done];
        }

        vec![Frame::Data(data)]
    }
}

/// Folds per-chunk events into a final stream: captures the in-band
/// `finish_reason`, accumulates usage, and synthesizes the terminal `Done`
/// event on the `[DONE]` sentinel.
pub struct CompletionState {
    finish_reason: Option<FinishReason>,
    usage: Option<Usage>,
    done: bool,
    missing_context: &'static str,
}

impl CompletionState {
    /// Creates a completion folder. `missing_context` names the provider in the
    /// decode error raised when the stream ends without a `finish_reason`.
    pub fn new(missing_context: &'static str) -> Self {
        Self {
            finish_reason: None,
            usage: None,
            done: false,
            missing_context,
        }
    }

    pub fn apply(&mut self, item: SseItem) -> Result<VecDeque<RawEvent>, Error> {
        let mut events = VecDeque::new();
        match item {
            SseItem::Events(raw_events) => {
                for event in raw_events {
                    match event {
                        RawEvent::Done {
                            finish_reason,
                            usage: _,
                        } => {
                            self.finish_reason = Some(finish_reason);
                        }
                        other => events.push_back(other),
                    }
                }
            }
            SseItem::Usage(usage) => {
                self.usage = Some(usage);
            }
            SseItem::Done => {
                let finish_reason = self
                    .finish_reason
                    .take()
                    .ok_or_else(|| missing_finish_reason(self.missing_context))?;
                events.push_back(RawEvent::Done {
                    finish_reason,
                    usage: self.usage.take(),
                });
                self.done = true;
            }
        }
        Ok(events)
    }

    pub fn is_done(&self) -> bool {
        self.done
    }
}

fn missing_finish_reason(context: &'static str) -> Error {
    Error::Decode {
        context: context.to_owned(),
        source: Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "SSE stream ended before a finish_reason chunk",
        )),
    }
}

/// One streamed Chat Completions chunk, generic over the per-provider `delta`
/// shape `D` and usage shape `U`.
#[derive(Deserialize)]
#[serde(bound(deserialize = "D: Deserialize<'de>, U: Deserialize<'de>"))]
pub struct Chunk<D, U> {
    #[serde(default = "Vec::new")]
    pub choices: Vec<Choice<D>>,
    pub usage: Option<U>,
}

#[derive(Deserialize)]
pub struct Choice<D> {
    pub delta: Option<D>,
    pub finish_reason: Option<String>,
}

#[derive(Deserialize)]
pub struct WireToolCall {
    pub index: u32,
    pub id: Option<String>,
    pub function: Option<WireFunction>,
}

#[derive(Deserialize)]
pub struct WireFunction {
    pub name: Option<String>,
    pub arguments: Option<String>,
}
