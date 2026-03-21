mod extension_host;

use std::path::Path;

use mimalloc::MiMalloc;
use wasmtime::Engine;

use extension_host::ExtensionInstance;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

fn main() -> wasmtime::Result<()> {
    let engine = Engine::default();
    // Built by `cargo build --release --target wasm32-wasip2` in the extension directory
    let wasm_path =
        Path::new("extensions/test-extension/target/wasm32-wasip2/release/test_extension.wasm");

    println!("Loading extension from {}", wasm_path.display());
    let mut extension = ExtensionInstance::load(&engine, wasm_path)?;

    let manifest = extension.register()?;
    println!(
        "Extension registered: id={}, name={}",
        manifest.id, manifest.name
    );

    println!("\nCalling tool 'hello'...");
    match extension.call_tool("hello", "{}")? {
        Ok(result) => println!("Tool result: {result}"),
        Err(err) => println!("Tool error: {err}"),
    }

    println!("\nCalling tool 'unknown'...");
    match extension.call_tool("unknown", "{}")? {
        Ok(result) => println!("Tool result: {result}"),
        Err(err) => println!("Tool error: {err}"),
    }

    Ok(())
}
