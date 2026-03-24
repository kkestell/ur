wit_bindgen::generate!({
    path: "../../../wit",
    world: "session-extension",
    generate_all,
});

use exports::ur::extension::extension::Guest as ExtGuest;
use exports::ur::extension::session_provider::Guest as SessionGuest;
use ur::extension::types::{
    ConfigEntry, ExtensionCapabilities, Message, SessionInfo, SettingDescriptor, ToolDescriptor,
};

struct SessionJsonl;

impl ExtGuest for SessionJsonl {
    fn init(_config: Vec<ConfigEntry>) -> Result<(), String> {
        Ok(())
    }

    fn call_tool(_name: String, _args_json: String) -> Result<String, String> {
        Err("no tools implemented".into())
    }

    fn id() -> String {
        "session-jsonl".into()
    }

    fn name() -> String {
        "Session JSONL".into()
    }

    fn list_tools() -> Vec<ToolDescriptor> {
        vec![]
    }

    fn list_settings() -> Vec<SettingDescriptor> {
        vec![]
    }

    fn declare_capabilities() -> ExtensionCapabilities {
        ExtensionCapabilities::empty()
    }
}

impl SessionGuest for SessionJsonl {
    fn load(_id: String) -> Result<Vec<Message>, String> {
        Ok(Vec::new())
    }

    fn append(_id: String, _msg: Message) -> Result<(), String> {
        Ok(())
    }

    fn list_sessions() -> Result<Vec<SessionInfo>, String> {
        Ok(Vec::new())
    }
}

export!(SessionJsonl);
