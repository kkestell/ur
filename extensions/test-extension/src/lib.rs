wit_bindgen::generate!({
    path: "../../wit",
    world: "ur-extension",
});

use exports::ur::extension::extension::{ExtensionManifest, Guest};

struct TestExtension;

impl Guest for TestExtension {
    fn register() -> ExtensionManifest {
        ExtensionManifest {
            id: "test".into(),
            name: "Test Extension".into(),
        }
    }

    fn call_tool(name: String, _args_json: String) -> Result<String, String> {
        match name.as_str() {
            "hello" => {
                ur::extension::host::log("hello was called on the guest — calling back to host");
                Ok("hello from test extension".into())
            }
            _ => Err(format!("unknown tool: {name}")),
        }
    }
}

export!(TestExtension);
