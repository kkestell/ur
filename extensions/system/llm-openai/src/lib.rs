wit_bindgen::generate!({
    path: "../../../wit",
    world: "llm-extension",
});

use exports::ur::extension::extension::Guest as ExtGuest;
use exports::ur::extension::llm_provider::Guest as LlmGuest;
use ur::extension::types::{
    Completion, ConfigEntry, ConfigSetting, Message, ModelDescriptor, SettingDescriptor,
    SettingEnum, SettingSchema, Usage,
};

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
    fn provider_id() -> String {
        "openai".into()
    }

    fn list_models() -> Vec<ModelDescriptor> {
        vec![ModelDescriptor {
            id: "gpt-5.4".into(),
            name: "GPT-5.4".into(),
            description: "Latest multimodal and reasoning model".into(),
            is_default: true,
            settings: vec![SettingDescriptor {
                key: "reasoning_effort".into(),
                name: "Reasoning Effort".into(),
                description: "How much effort to spend reasoning".into(),
                schema: SettingSchema::Enumeration(SettingEnum {
                    allowed: vec!["low".into(), "medium".into(), "high".into()],
                    default_val: "medium".into(),
                }),
            }],
        }]
    }

    fn complete(
        messages: Vec<Message>,
        _model: String,
        _settings: Vec<ConfigSetting>,
    ) -> Result<Completion, String> {
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
