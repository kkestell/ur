wit_bindgen::generate!({
    path: "../../../wit",
    world: "llm-extension",
});

use exports::ur::extension::extension::Guest as ExtGuest;
use exports::ur::extension::llm_provider::Guest as LlmGuest;
use ur::extension::types::{
    Completion, ConfigEntry, ConfigSetting, Message, ModelDescriptor, SettingDescriptor,
    SettingInteger, SettingSchema, Usage,
};

struct LlmAnthropic;

impl ExtGuest for LlmAnthropic {
    fn init(_config: Vec<ConfigEntry>) -> Result<(), String> {
        Ok(())
    }

    fn call_tool(_name: String, _args_json: String) -> Result<String, String> {
        Err("no tools implemented".into())
    }
}

impl LlmGuest for LlmAnthropic {
    fn provider_id() -> String {
        "anthropic".into()
    }

    fn list_models() -> Vec<ModelDescriptor> {
        vec![ModelDescriptor {
            id: "claude-sonnet-4-6".into(),
            name: "Claude Sonnet 4.6".into(),
            description: "Balanced performance and cost".into(),
            is_default: true,
            settings: vec![SettingDescriptor {
                key: "thinking_budget".into(),
                name: "Thinking Budget".into(),
                description: "Token budget for extended thinking".into(),
                schema: SettingSchema::Integer(SettingInteger {
                    min: 0,
                    max: 128_000,
                    default_val: 4_000,
                }),
            }],
        }]
    }

    fn complete(
        messages: Vec<Message>,
        _model: String,
        _settings: Vec<ConfigSetting>,
    ) -> Result<Completion, String> {
        let reply = format!("anthropic stub: received {} messages", messages.len());
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

export!(LlmAnthropic);
