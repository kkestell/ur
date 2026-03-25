//! Tool-dispatch tests for extension capabilities.
//!
//! Tests the Lua host API (tool handlers, async HTTP, etc.) by loading
//! extensions directly — no CLI involved.

use std::io::Write;
use std::sync::Arc;

use ur::host_api::HostProviders;
use ur::lua_host::LuaExtension;
use ur::types::ExtensionCapabilities;

/// Starts a minimal HTTP server on a random port and returns the address.
///
/// The server responds to any request with HTTP 200 and a small body.
/// It accepts exactly `request_count` connections, then shuts down.
fn spawn_http_server(request_count: usize) -> std::net::SocketAddr {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind to random port");
    let addr = listener.local_addr().expect("get local addr");

    std::thread::spawn(move || {
        for _ in 0..request_count {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 1024];
                let _ = std::io::Read::read(&mut stream, &mut buf);

                let body = "ok";
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
            }
        }
    });

    addr
}

/// Creates a temp Lua extension with network capability and the given source.
fn temp_extension_with_network(lua_source: &str) -> (tempfile::TempDir, Arc<LuaExtension>) {
    let dir = tempfile::tempdir().unwrap();
    let init_path = dir.path().join("init.lua");
    std::fs::write(&init_path, lua_source).unwrap();

    let caps = ExtensionCapabilities {
        network: true,
        fs_read: false,
        fs_write: false,
    };
    let ext = LuaExtension::load(
        dir.path(),
        "test-http-ext",
        "Test HTTP Extension",
        &caps,
        &serde_json::json!({}),
        &HostProviders::default(),
    )
    .unwrap();
    (dir, Arc::new(ext))
}

/// Proves that `ur.http.get()` works from a Lua tool handler,
/// exercising the full async dispatch path:
///
///   Lua handler → `ur.http.get()` → async reqwest → tokio runtime → response back to Lua
///
/// Uses a local HTTP server to avoid external network dependencies.
#[tokio::test(flavor = "multi_thread")]
async fn lua_tool_handler_can_call_http_get() {
    let addr = spawn_http_server(1);
    let url = format!("http://{addr}/test");

    let lua_source = r#"
        ur.tool("http_check", {
            description = "Fetch a URL and return status",
            parameters = {
                type = "object",
                properties = {
                    url = { type = "string" },
                },
            },
            handler = function(args)
                local response = ur.http.get(args.url)
                return { status = response.status, body = response.body }
            end,
        })
    "#;

    let (_dir, ext) = temp_extension_with_network(lua_source);

    let args_json = serde_json::json!({ "url": url }).to_string();
    let result = ext.call_tool("http_check", &args_json).unwrap();

    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed["status"], 200, "HTTP status should be 200");
    assert_eq!(parsed["body"], "ok", "body should match server response");
}

/// Proves that HTTP errors are propagated correctly back to Lua.
#[tokio::test(flavor = "multi_thread")]
async fn lua_tool_handler_http_get_propagates_status_codes() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");

    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 1024];
            let _ = std::io::Read::read(&mut stream, &mut buf);

            let body = "not found";
            let response = format!(
                "HTTP/1.1 404 Not Found\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });

    let url = format!("http://{addr}/missing");
    let lua_source = r#"
        ur.tool("http_check", {
            description = "Fetch a URL and return status",
            parameters = {
                type = "object",
                properties = {
                    url = { type = "string" },
                },
            },
            handler = function(args)
                local response = ur.http.get(args.url)
                return { status = response.status, body = response.body }
            end,
        })
    "#;

    let (_dir, ext) = temp_extension_with_network(lua_source);
    let args_json = serde_json::json!({ "url": url }).to_string();
    let result = ext.call_tool("http_check", &args_json).unwrap();

    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed["status"], 404, "HTTP status should be 404");
}

/// Proves that the test extension's `http_status` tool works end-to-end.
#[tokio::test(flavor = "multi_thread")]
async fn http_status_tool() {
    let addr = spawn_http_server(1);
    let url = format!("http://{addr}/status-check");

    let test_ext_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("extensions/workspace/test-extension");

    let caps = ExtensionCapabilities {
        network: true,
        fs_read: false,
        fs_write: false,
    };
    let ext = LuaExtension::load(
        &test_ext_dir,
        "test-extension",
        "Test Extension",
        &caps,
        &serde_json::json!({}),
        &HostProviders::default(),
    )
    .unwrap();

    let descriptors = ext.tool_descriptors();
    assert!(
        descriptors.iter().any(|d| d.name == "http_status"),
        "test extension should register http_status tool"
    );

    let args_json = serde_json::json!({ "url": url }).to_string();
    let result = ext.call_tool("http_status", &args_json).unwrap();

    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(
        parsed["status"], 200,
        "http_status tool should return 200 for local server"
    );
}
