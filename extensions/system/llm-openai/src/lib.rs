wit_bindgen::generate!({
    path: "../../../wit",
    world: "llm-extension",
});

use exports::ur::extension::extension::Guest as ExtGuest;
use exports::ur::extension::llm_provider::Guest as LlmGuest;
use ur::extension::types::{CompleteOpts, Completion, ConfigEntry, Message, Usage};

struct LlmOpenai;

impl ExtGuest for LlmOpenai {
    fn init(_config: Vec<ConfigEntry>) -> Result<(), String> {
        Ok(())
    }

    fn call_tool(_name: String, _args_json: String) -> Result<String, String> {
        Err("no tools implemented".into())
    }
}

impl LlmGuest for LlmOpenai {
    fn complete(messages: Vec<Message>, _opts: Option<CompleteOpts>) -> Result<Completion, String> {
        let reply = format!("openai stub: received {} messages", messages.len());
        Ok(Completion {
            message: Message {
                role: "assistant".into(),
                content: reply,
            },
            usage: Some(Usage {
                input_tokens: 0,
                output_tokens: 0,
            }),
        })
    }
}

export!(LlmOpenai);

// Rust guideline compliant 2026-02-21
