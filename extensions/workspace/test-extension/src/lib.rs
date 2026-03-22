wit_bindgen::generate!({
    path: "../../../wit",
    world: "general-extension",
});

use exports::ur::extension::extension::Guest as ExtGuest;
use ur::extension::types::{ConfigEntry, ToolDescriptor};

struct TestExtension;

impl ExtGuest for TestExtension {
    fn init(_config: Vec<ConfigEntry>) -> Result<(), String> {
        Ok(())
    }

    fn call_tool(name: String, args_json: String) -> Result<String, String> {
        match name.as_str() {
            "greet" => Ok(format!("Hello, world! (args: {args_json})")),
            other => Err(format!("unknown tool: {other}")),
        }
    }

    fn id() -> String {
        "test-extension".into()
    }

    fn name() -> String {
        "Test Extension".into()
    }

    fn list_tools() -> Vec<ToolDescriptor> {
        vec![ToolDescriptor {
            name: "greet".into(),
            description: "Greet someone by name".into(),
            parameters_json_schema: r#"{"type":"object","properties":{"name":{"type":"string","description":"Name to greet"}},"required":["name"]}"#.into(),
        }]
    }
}

export!(TestExtension);
