//! SSE parsing and DeepSeek chunk normalization.

use std::collections::VecDeque;

use serde::Deserialize;
use ur_core::Error;
use ur_core::event::{FinishReason, Usage};
use ur_core::provider::RawEvent;

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum SseItem {
    Events(Vec<RawEvent>),
    Usage(Usage),
    Done,
}

#[derive(Default)]
pub(crate) struct SseDecoder {
    buffer: Vec<u8>,
    data_lines: Vec<String>,
}

impl SseDecoder {
    pub(crate) fn push(&mut self, bytes: &[u8]) -> Result<Vec<SseItem>, Error> {
        self.buffer.extend_from_slice(bytes);
        self.drain_lines()
    }

    pub(crate) fn finish(&mut self) -> Result<Vec<SseItem>, Error> {
        let mut items = Vec::new();
        if !self.buffer.is_empty() {
            let mut line = std::mem::take(&mut self.buffer);
            if line.ends_with(b"\r") {
                line.pop();
            }
            items.extend(self.process_line_bytes(&line)?);
        }

        if !self.data_lines.is_empty() {
            items.extend(self.dispatch_event()?);
        }

        Ok(items)
    }

    fn drain_lines(&mut self) -> Result<Vec<SseItem>, Error> {
        let mut items = Vec::new();
        while let Some(position) = self.buffer.iter().position(|byte| *byte == b'\n') {
            let mut line = self.buffer.drain(..=position).collect::<Vec<_>>();
            line.pop();
            if line.ends_with(b"\r") {
                line.pop();
            }
            items.extend(self.process_line_bytes(&line)?);
        }
        Ok(items)
    }

    fn process_line_bytes(&mut self, line: &[u8]) -> Result<Vec<SseItem>, Error> {
        let line = std::str::from_utf8(line).map_err(|source| Error::Decode {
            context: "reading SSE line".to_owned(),
            source: Box::new(source),
        })?;
        self.process_line(line)
    }

    fn process_line(&mut self, line: &str) -> Result<Vec<SseItem>, Error> {
        if line.is_empty() {
            return self.dispatch_event();
        }

        if line.starts_with(':') {
            return Ok(Vec::new());
        }

        let Some(data) = line.strip_prefix("data:") else {
            return Ok(Vec::new());
        };
        self.data_lines
            .push(data.strip_prefix(' ').unwrap_or(data).to_owned());
        Ok(Vec::new())
    }

    fn dispatch_event(&mut self) -> Result<Vec<SseItem>, Error> {
        if self.data_lines.is_empty() {
            return Ok(Vec::new());
        }

        let data = std::mem::take(&mut self.data_lines).join("\n");
        if data == "[DONE]" {
            return Ok(vec![SseItem::Done]);
        }

        decode_chunk(&data)
    }
}

#[derive(Default)]
pub(crate) struct CompletionState {
    finish_reason: Option<FinishReason>,
    usage: Option<Usage>,
    done: bool,
}

impl CompletionState {
    pub(crate) fn apply(&mut self, item: SseItem) -> Result<VecDeque<RawEvent>, Error> {
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
                    .ok_or_else(missing_finish_reason)?;
                events.push_back(RawEvent::Done {
                    finish_reason,
                    usage: self.usage.take(),
                });
                self.done = true;
            }
        }
        Ok(events)
    }

    pub(crate) fn is_done(&self) -> bool {
        self.done
    }
}

fn decode_chunk(data: &str) -> Result<Vec<SseItem>, Error> {
    let chunk: Chunk = serde_json::from_str(data).map_err(|source| Error::Decode {
        context: "decoding DeepSeek SSE chunk".to_owned(),
        source: Box::new(source),
    })?;

    let mut items = Vec::new();
    let mut events = Vec::new();

    for choice in chunk.choices {
        if let Some(delta) = choice.delta {
            if let Some(content) = delta.content {
                events.push(RawEvent::TextDelta(content));
            }
            if let Some(reasoning_content) = delta.reasoning_content {
                events.push(RawEvent::ReasoningDelta(reasoning_content));
            }
            for call in delta.tool_calls.unwrap_or_default() {
                events.push(RawEvent::ToolCallDelta {
                    index: call.index,
                    id: call.id,
                    name: call
                        .function
                        .as_ref()
                        .and_then(|function| function.name.clone()),
                    arguments: call
                        .function
                        .and_then(|function| function.arguments)
                        .unwrap_or_default(),
                });
            }
        }

        if let Some(reason) = choice.finish_reason {
            events.push(RawEvent::Done {
                finish_reason: finish_reason(&reason),
                usage: None,
            });
        }
    }

    if !events.is_empty() {
        items.push(SseItem::Events(events));
    }
    if let Some(usage) = chunk.usage {
        items.push(SseItem::Usage(usage.into()));
    }

    Ok(items)
}

fn finish_reason(reason: &str) -> FinishReason {
    match reason {
        "stop" => FinishReason::Stop,
        "length" => FinishReason::Length,
        "content_filter" => FinishReason::ContentFilter,
        "tool_calls" => FinishReason::ToolCalls,
        other => FinishReason::Other(other.to_owned()),
    }
}

fn missing_finish_reason() -> Error {
    Error::Decode {
        context: "finishing DeepSeek SSE stream".to_owned(),
        source: Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "SSE stream ended before a finish_reason chunk",
        )),
    }
}

#[derive(Deserialize)]
struct Chunk {
    #[serde(default)]
    choices: Vec<Choice>,
    usage: Option<WireUsage>,
}

#[derive(Deserialize)]
struct Choice {
    delta: Option<Delta>,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct Delta {
    content: Option<String>,
    reasoning_content: Option<String>,
    tool_calls: Option<Vec<WireToolCall>>,
}

#[derive(Deserialize)]
struct WireToolCall {
    index: u32,
    id: Option<String>,
    function: Option<WireFunction>,
}

#[derive(Deserialize)]
struct WireFunction {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Deserialize)]
struct WireUsage {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
    #[serde(default)]
    total_tokens: u32,
    prompt_cache_hit_tokens: Option<u32>,
    completion_tokens_details: Option<CompletionTokensDetails>,
}

impl From<WireUsage> for Usage {
    fn from(value: WireUsage) -> Self {
        let mut usage = Self::default();
        usage.prompt_tokens = value.prompt_tokens;
        usage.completion_tokens = value.completion_tokens;
        usage.total_tokens = value.total_tokens;
        usage.cached_prompt_tokens = value.prompt_cache_hit_tokens;
        usage.reasoning_tokens = value
            .completion_tokens_details
            .and_then(|details| details.reasoning_tokens);
        usage
    }
}

#[derive(Deserialize)]
struct CompletionTokensDetails {
    reasoning_tokens: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn decode(input: &str) -> Result<Vec<RawEvent>, Error> {
        let mut decoder = SseDecoder::default();
        let mut state = CompletionState::default();
        let mut events = Vec::new();
        for item in decoder.push(input.as_bytes())? {
            events.extend(state.apply(item)?);
        }
        for item in decoder.finish()? {
            events.extend(state.apply(item)?);
        }
        Ok(events)
    }

    fn decode_chunks(chunks: &[&[u8]]) -> Result<Vec<RawEvent>, Error> {
        let mut decoder = SseDecoder::default();
        let mut state = CompletionState::default();
        let mut events = Vec::new();
        for chunk in chunks {
            for item in decoder.push(chunk)? {
                events.extend(state.apply(item)?);
            }
        }
        for item in decoder.finish()? {
            events.extend(state.apply(item)?);
        }
        Ok(events)
    }

    fn event(data: serde_json::Value) -> String {
        format!("data: {data}\n\n")
    }

    #[test]
    fn decodes_text_reasoning_usage_and_done() {
        let input = format!(
            ": keep-alive\n\n{}{}{}data: [DONE]\n\n",
            event(json!({
                "choices": [{
                    "delta": { "content": "Hi", "reasoning_content": "Thinking" },
                    "finish_reason": null
                }],
                "usage": null
            })),
            event(json!({
                "choices": [{
                    "delta": {},
                    "finish_reason": "stop"
                }],
                "usage": null
            })),
            event(json!({
                "choices": [],
                "usage": {
                    "prompt_tokens": 3,
                    "completion_tokens": 4,
                    "total_tokens": 7,
                    "prompt_cache_hit_tokens": 2,
                    "completion_tokens_details": { "reasoning_tokens": 1 }
                }
            })),
        );

        let events = decode(&input).unwrap();
        let mut usage = Usage::default();
        usage.prompt_tokens = 3;
        usage.completion_tokens = 4;
        usage.total_tokens = 7;
        usage.cached_prompt_tokens = Some(2);
        usage.reasoning_tokens = Some(1);

        assert_eq!(
            events,
            vec![
                RawEvent::TextDelta("Hi".to_owned()),
                RawEvent::ReasoningDelta("Thinking".to_owned()),
                RawEvent::Done {
                    finish_reason: FinishReason::Stop,
                    usage: Some(usage),
                },
            ]
        );
    }

    #[test]
    fn decodes_multi_fragment_tool_calls_and_unknown_finish_reason() {
        let input = format!(
            "{}{}data: [DONE]\n\n",
            event(json!({
                "choices": [{
                    "delta": {
                        "tool_calls": [{
                            "index": 0,
                            "id": "call-1",
                            "function": { "name": "add", "arguments": "{\"a\"" }
                        }]
                    },
                    "finish_reason": null
                }],
                "usage": null
            })),
            event(json!({
                "choices": [{
                    "delta": {
                        "tool_calls": [{
                            "index": 0,
                            "function": { "arguments": ":1}" }
                        }]
                    },
                    "finish_reason": "insufficient_system_resource"
                }],
                "usage": null
            })),
        );

        let events = decode(&input).unwrap();
        assert_eq!(
            events,
            vec![
                RawEvent::ToolCallDelta {
                    index: 0,
                    id: Some("call-1".to_owned()),
                    name: Some("add".to_owned()),
                    arguments: "{\"a\"".to_owned(),
                },
                RawEvent::ToolCallDelta {
                    index: 0,
                    id: None,
                    name: None,
                    arguments: ":1}".to_owned(),
                },
                RawEvent::Done {
                    finish_reason: FinishReason::Other("insufficient_system_resource".to_owned()),
                    usage: None,
                },
            ]
        );
    }

    #[test]
    fn malformed_json_is_a_decode_error() {
        let error = decode("data: {not json}\n\n").unwrap_err();
        assert!(matches!(error, Error::Decode { .. }));
    }

    #[test]
    fn split_utf8_and_missing_final_blank_line_are_accepted() {
        let input = format!(
            "{}{}data: [DONE]",
            event(json!({
                "choices": [{
                    "delta": { "content": "café" },
                    "finish_reason": null
                }],
                "usage": null
            })),
            event(json!({
                "choices": [{ "delta": {}, "finish_reason": "stop" }],
                "usage": null
            })),
        );
        let split = input.find("é").unwrap() + 1;
        let events = decode_chunks(&[&input.as_bytes()[..split], &input.as_bytes()[split..]])
            .expect("split unicode chunk decodes");

        assert_eq!(
            events,
            vec![
                RawEvent::TextDelta("café".to_owned()),
                RawEvent::Done {
                    finish_reason: FinishReason::Stop,
                    usage: None,
                },
            ]
        );
    }

    #[test]
    fn done_without_finish_reason_is_a_decode_error() {
        let error = decode("data: [DONE]\n\n").unwrap_err();
        assert!(matches!(error, Error::Decode { .. }));
    }

    #[test]
    fn usage_without_finish_reason_is_a_decode_error() {
        let input = format!(
            "{}data: [DONE]\n\n",
            event(json!({
                "choices": [],
                "usage": {
                    "prompt_tokens": 1,
                    "completion_tokens": 2,
                    "total_tokens": 3
                }
            })),
        );
        let error = decode(&input).unwrap_err();
        assert!(matches!(error, Error::Decode { .. }));
    }
}
