wit_bindgen::generate!({
    path: "../../../wit",
    world: "llm-extension",
});

use std::cell::RefCell;

use exports::ur::extension::extension::Guest as ExtGuest;
use exports::ur::extension::llm_provider::Guest as LlmGuest;
use exports::ur::extension::llm_streaming_provider::{
    CompletionStream, Guest as LlmStreamingGuest, GuestCompletionStream,
};
use ur::extension::types::{
    Completion, CompletionChunk, ConfigEntry, ConfigSetting, Message, MessagePart,
    ModelDescriptor, ToolCall, ToolDescriptor, Usage,
};

struct LlmTest;

// ── Extension lifecycle ──────────────────────────────────────────────

impl ExtGuest for LlmTest {
    fn init(_config: Vec<ConfigEntry>) -> Result<(), String> {
        Ok(())
    }

    fn call_tool(_name: String, _args_json: String) -> Result<String, String> {
        Err("no tools implemented".into())
    }

    fn id() -> String {
        "llm-test".into()
    }

    fn name() -> String {
        "Test LLM".into()
    }

    fn list_tools() -> Vec<ToolDescriptor> {
        vec![]
    }
}

// ── LLM provider ────────────────────────────────────────────────────

impl LlmGuest for LlmTest {
    fn provider_id() -> String {
        "test".into()
    }

    fn list_models() -> Vec<ModelDescriptor> {
        vec![ModelDescriptor {
            id: "echo".into(),
            name: "Echo".into(),
            description: "Deterministic test model".into(),
            is_default: true,
            settings: vec![],
        }]
    }

    fn complete(
        messages: Vec<Message>,
        _model: String,
        _settings: Vec<ConfigSetting>,
        tools: Vec<ToolDescriptor>,
    ) -> Result<Completion, String> {
        let message = deterministic_response(&messages, &tools);
        Ok(Completion {
            message,
            usage: Some(Usage {
                input_tokens: 1,
                output_tokens: 1,
            }),
        })
    }
}

// ── Streaming provider ──────────────────────────────────────────────

struct TestStream {
    inner: RefCell<Option<CompletionChunk>>,
}

impl LlmStreamingGuest for LlmTest {
    type CompletionStream = TestStream;

    fn complete_streaming(
        messages: Vec<Message>,
        _model: String,
        _settings: Vec<ConfigSetting>,
        tools: Vec<ToolDescriptor>,
    ) -> Result<CompletionStream, String> {
        let message = deterministic_response(&messages, &tools);
        let chunk = CompletionChunk {
            delta_parts: message.parts,
            finish_reason: Some("stop".into()),
            usage: Some(Usage {
                input_tokens: 1,
                output_tokens: 1,
            }),
        };
        Ok(CompletionStream::new(TestStream {
            inner: RefCell::new(Some(chunk)),
        }))
    }
}

impl GuestCompletionStream for TestStream {
    fn next(&self) -> Option<CompletionChunk> {
        self.inner.borrow_mut().take()
    }
}

// ── Deterministic logic ─────────────────────────────────────────────

fn has_tool_result(messages: &[Message]) -> bool {
    messages.iter().any(|m| {
        m.parts
            .iter()
            .any(|p| matches!(p, MessagePart::ToolResult(_)))
    })
}

fn last_user_text(messages: &[Message]) -> String {
    messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map(|m| {
            m.parts
                .iter()
                .filter_map(|p| match p {
                    MessagePart::Text(s) => Some(s.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default()
}

fn deterministic_response(messages: &[Message], tools: &[ToolDescriptor]) -> Message {
    // If tools are declared and we haven't seen a tool result yet,
    // call the first tool with dummy args.
    if !tools.is_empty() && !has_tool_result(messages) {
        let tool = &tools[0];
        return Message {
            role: "assistant".into(),
            parts: vec![MessagePart::ToolCall(ToolCall {
                id: "call-1".into(),
                name: tool.name.clone(),
                arguments_json: r#"{"name":"world"}"#.into(),
                provider_metadata_json: String::new(),
            })],
        };
    }

    // If we have a tool result, summarize it.
    if has_tool_result(messages) {
        let content = messages
            .iter()
            .rev()
            .flat_map(|m| &m.parts)
            .find_map(|p| match p {
                MessagePart::ToolResult(tr) => Some(tr.content.clone()),
                _ => None,
            })
            .unwrap_or_default();
        return Message {
            role: "assistant".into(),
            parts: vec![MessagePart::Text(format!("Tool result received: {content}"))],
        };
    }

    // No tools — echo the last user message.
    Message {
        role: "assistant".into(),
        parts: vec![MessagePart::Text(format!(
            "Echo: {}",
            last_user_text(messages)
        ))],
    }
}

export!(LlmTest);
