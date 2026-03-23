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
            "get_weather" => {
                let location = extract_location(&args_json)
                    .ok_or_else(|| format!("missing location in args: {args_json}"))?;
                Ok(weather_forecast(location))
            }
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
            name: "get_weather".into(),
            description: "Return a weather forecast for a location".into(),
            parameters_json_schema: r#"{"type":"object","properties":{"location":{"type":"string","description":"City or place to forecast"}},"required":["location"]}"#.into(),
        }]
    }
}

fn extract_location(args_json: &str) -> Option<&str> {
    let field = "\"location\"";
    let after_field = args_json.split_once(field)?.1;
    let after_colon = after_field.split_once(':')?.1.trim_start();
    let quoted = after_colon.strip_prefix('"')?;
    let end = quoted.find('"')?;
    Some(&quoted[..end])
}

fn weather_forecast(location: &str) -> String {
    format!("{location}: cool and cloudy, 12C, with a light breeze. A coat would be a good idea.")
}

export!(TestExtension);
