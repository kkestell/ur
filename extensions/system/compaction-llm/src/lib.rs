wit_bindgen::generate!({
    path: "../../../wit",
    world: "compaction-extension",
});

use exports::ur::extension::compaction_provider::Guest as CompactionGuest;
use exports::ur::extension::extension::Guest as ExtGuest;
use ur::extension::types::{ConfigEntry, Message};

struct CompactionLlm;

impl ExtGuest for CompactionLlm {
    fn init(_config: Vec<ConfigEntry>) -> Result<(), String> {
        Ok(())
    }

    fn call_tool(_name: String, _args_json: String) -> Result<String, String> {
        Err("no tools implemented".into())
    }
}

impl CompactionGuest for CompactionLlm {
    fn compact(messages: Vec<Message>) -> Result<Vec<Message>, String> {
        // Stub: return messages unchanged.
        Ok(messages)
    }
}

export!(CompactionLlm);

// Rust guideline compliant 2026-02-21
