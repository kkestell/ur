wit_bindgen::generate!({
    path: "../../../wit",
    world: "compaction-extension",
});

use exports::ur::extension::compaction_provider::Guest as CompactionGuest;
use exports::ur::extension::extension::Guest as ExtGuest;
use ur::extension::types::{ConfigEntry, Message, ToolDescriptor};

struct CompactionLlm;

impl ExtGuest for CompactionLlm {
    fn init(_config: Vec<ConfigEntry>) -> Result<(), String> {
        Ok(())
    }

    fn call_tool(_name: String, _args_json: String) -> Result<String, String> {
        Err("no tools implemented".into())
    }

    fn id() -> String {
        "compaction-llm".into()
    }

    fn name() -> String {
        "Compaction LLM".into()
    }

    fn list_tools() -> Vec<ToolDescriptor> {
        vec![]
    }
}

impl CompactionGuest for CompactionLlm {
    fn compact(messages: Vec<Message>) -> Result<Vec<Message>, String> {
        // Stub: return messages unchanged.
        Ok(messages)
    }
}

export!(CompactionLlm);
