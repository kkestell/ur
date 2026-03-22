wit_bindgen::generate!({
    path: "../../../wit",
    world: "general-extension",
});

use exports::ur::extension::extension::Guest as ExtGuest;
use ur::extension::types::ConfigEntry;

struct TestExtension;

impl ExtGuest for TestExtension {
    fn init(_config: Vec<ConfigEntry>) -> Result<(), String> {
        Ok(())
    }

    fn call_tool(_name: String, _args_json: String) -> Result<String, String> {
        Err("no tools implemented".into())
    }
}

export!(TestExtension);
