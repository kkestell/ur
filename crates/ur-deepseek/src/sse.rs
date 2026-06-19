//! DeepSeek chunk normalization over the shared SSE framing.

use serde::Deserialize;
use ur_core::Error;
use ur_core::event::Usage;
use ur_core::provider::RawEvent;
use ur_openai_compat::sse::{
    Chunk, CompletionTokensDetails, SseItem, WireFunction, WireToolCall, finish_reason,
};

pub(crate) fn decode_chunk(data: &str) -> Result<Vec<SseItem>, Error> {
    let chunk: Chunk<Delta, WireUsage> =
        serde_json::from_str(data).map_err(|source| Error::Decode {
            context: "decoding DeepSeek SSE chunk".to_owned(),
            source: Box::new(source),
        })?;

    let mut items = Vec::new();
    let mut events = Vec::new();
    let mut finish = None;

    for choice in chunk.choices {
        if let Some(delta) = choice.delta {
            if let Some(content) = delta.content {
                events.push(RawEvent::TextDelta(content));
            }
            if let Some(reasoning_content) = delta.reasoning_content {
                events.push(RawEvent::ReasoningDelta(reasoning_content));
            }
            for call in delta.tool_calls.unwrap_or_default() {
                let (name, arguments) = match call.function {
                    Some(WireFunction { name, arguments }) => (name, arguments.unwrap_or_default()),
                    None => (None, String::new()),
                };
                events.push(RawEvent::ToolCallDelta {
                    index: call.index,
                    id: call.id,
                    name,
                    arguments,
                });
            }
        }

        if let Some(reason) = choice.finish_reason {
            finish = Some(finish_reason(&reason, false));
        }
    }

    if !events.is_empty() {
        items.push(SseItem::Events(events));
    }
    if let Some(reason) = finish {
        items.push(SseItem::FinishReason(reason));
    }
    if let Some(usage) = chunk.usage {
        items.push(SseItem::Usage(usage.into()));
    }

    Ok(items)
}

#[derive(Deserialize)]
struct Delta {
    content: Option<String>,
    reasoning_content: Option<String>,
    tool_calls: Option<Vec<WireToolCall>>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use ur_core::event::FinishReason;
    use ur_openai_compat::sse::{CompletionState, SseDecoder};
    use ur_openai_compat::test_support::drive;

    fn decode(input: &str) -> Result<Vec<RawEvent>, Error> {
        decode_chunks(&[input.as_bytes()])
    }

    fn decode_chunks(chunks: &[&[u8]]) -> Result<Vec<RawEvent>, Error> {
        let mut decoder = SseDecoder::default();
        let mut state = CompletionState::new("DeepSeek");
        let mut events = Vec::new();
        for chunk in chunks {
            let frames = decoder.push(chunk)?;
            drive(decode_chunk, &mut state, &mut events, frames)?;
        }
        let frames = decoder.finish()?;
        drive(decode_chunk, &mut state, &mut events, frames)?;
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
