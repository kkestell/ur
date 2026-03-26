//! Tool-dispatch tests for the read-file system extension.

use std::fs;

use ur::host_api::HostProviders;
use ur::lua_host::LuaExtension;
use ur::types::ExtensionCapabilities;

fn load_read_file_extension() -> LuaExtension {
    let ext_dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("extensions/system/read-file");

    let caps = ExtensionCapabilities {
        network: false,
        fs_read: true,
        fs_write: false,
    };
    LuaExtension::load(
        &ext_dir,
        "read-file",
        "Read File",
        &caps,
        &serde_json::json!({}),
        &HostProviders::default(),
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn registers_read_file_tool() {
    let ext = load_read_file_extension();
    let descriptors = ext.tool_descriptors();
    assert!(
        descriptors.iter().any(|d| d.name == "read_file"),
        "should register read_file tool"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn returns_file_contents() {
    let ext = load_read_file_extension();

    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("hello.txt");
    fs::write(&file_path, "hello world\n").unwrap();

    let args = serde_json::json!({ "path": file_path.to_str().unwrap() }).to_string();
    let result = ext.call_tool("read_file", &args).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();

    assert_eq!(parsed["content"], "hello world\n");
}

#[tokio::test(flavor = "multi_thread")]
async fn offset_and_limit() {
    let ext = load_read_file_extension();

    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("lines.txt");
    fs::write(&file_path, "line1\nline2\nline3\nline4\nline5\n").unwrap();

    // Read lines 2-3 (offset=2, limit=2).
    let args = serde_json::json!({
        "path": file_path.to_str().unwrap(),
        "offset": 2,
        "limit": 2
    })
    .to_string();
    let result = ext.call_tool("read_file", &args).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();

    assert_eq!(parsed["content"], "line2\nline3\n");
}

#[tokio::test(flavor = "multi_thread")]
async fn truncates_large_content() {
    let ext = load_read_file_extension();

    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("big.txt");
    // Each line is 100 bytes; 2000 lines = 200 KB > 128 KB threshold.
    let line = "x".repeat(99) + "\n";
    let content = line.repeat(2000);
    fs::write(&file_path, &content).unwrap();

    let args = serde_json::json!({ "path": file_path.to_str().unwrap() }).to_string();
    let result = ext.call_tool("read_file", &args).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();

    let text = parsed["content"].as_str().unwrap();
    assert!(
        text.contains("[truncated"),
        "should contain truncation message"
    );
    assert!(
        text.contains("Use offset/limit to read in chunks"),
        "should hint at using offset/limit"
    );
    let truncation_marker = text.find("\n[truncated").unwrap();
    assert!(
        truncation_marker <= 128 * 1024,
        "truncated content should be <= 128 KB"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn returns_error_for_missing_file() {
    let ext = load_read_file_extension();

    let args = serde_json::json!({ "path": "/nonexistent/file.txt" }).to_string();
    let result = ext.call_tool("read_file", &args).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();

    assert!(
        parsed["error"].as_str().is_some(),
        "should return an error for missing file"
    );
}
