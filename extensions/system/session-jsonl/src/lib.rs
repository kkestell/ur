wit_bindgen::generate!({
    path: "../../../wit",
    world: "session-extension",
    generate_all,
});

use std::io::{BufRead, BufReader, Write};

use exports::ur::extension::extension::Guest as ExtGuest;
use exports::ur::extension::session_provider::Guest as SessionGuest;
use ur::extension::types::{
    ApprovalDecision, ConfigEntry, ExtensionCapabilities, Message, MessagePart, SessionEvent,
    SessionInfo, SettingDescriptor, ToolApprovalDecisionRecord, ToolApprovalRequest, ToolCall,
    ToolDescriptor, ToolResult, TurnInterruption,
};

mod serde_types;

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
        ExtensionCapabilities::FILESYSTEM_READ | ExtensionCapabilities::FILESYSTEM_WRITE
    }
}

fn session_path(id: &str) -> String {
    format!("/data/{id}.jsonl")
}

impl SessionGuest for SessionJsonl {
    fn load(id: String) -> Result<Vec<SessionEvent>, String> {
        let path = session_path(&id);
        let file = match std::fs::File::open(&path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(format!("open {path}: {e}")),
        };
        let reader = BufReader::new(file);
        let mut events = Vec::new();
        for line in reader.lines() {
            let line = line.map_err(|e| format!("read line: {e}"))?;
            if line.trim().is_empty() {
                continue;
            }
            let serde_event: serde_types::SerdeSessionEvent =
                serde_json::from_str(&line).map_err(|e| format!("parse event: {e}"))?;
            events.push(serde_event.into_wit());
        }
        Ok(events)
    }

    fn append(id: String, event: SessionEvent) -> Result<(), String> {
        let path = session_path(&id);
        let serde_event = serde_types::SerdeSessionEvent::from_wit(&event);
        let json = serde_json::to_string(&serde_event).map_err(|e| format!("serialize: {e}"))?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| format!("open {path}: {e}"))?;
        writeln!(file, "{json}").map_err(|e| format!("write: {e}"))?;
        Ok(())
    }

    fn list_sessions() -> Result<Vec<SessionInfo>, String> {
        let entries = match std::fs::read_dir("/data") {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(format!("read /data: {e}")),
        };
        let mut sessions = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| format!("read entry: {e}"))?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if let Some(id) = name.strip_suffix(".jsonl") {
                sessions.push(SessionInfo {
                    id: id.to_string(),
                    title: None,
                });
            }
        }
        Ok(sessions)
    }
}

export!(SessionJsonl);
