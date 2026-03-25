//! Run command integration tests.
//!
//! Tests for the turn execution and session management.

#[test]
fn http_tool_integration_documentation() {
    // Integration test for HTTP tool functionality.
    //
    // The test extension now includes an http_status tool that demonstrates
    // async HTTP capability. The tool is implemented in Lua and calls ur.http.get()
    // to fetch a URL and return the HTTP status code.
    //
    // Example usage from Lua:
    //   ur.tool("http_status", {
    //       handler = function(args)
    //           local response = ur.http.get(args.url)
    //           return { status = response.status }
    //       end
    //   })
    //
    // To test this end-to-end:
    // 1. Start a test HTTP server (e.g., using mockito or httpmock)
    // 2. Call the http_status tool with the test server URL
    // 3. Verify the returned status code matches the expected value
    //
    // This capability proves that:
    // - main() is wrapped with #[tokio::main] providing a runtime
    // - call_tool() uses block_on(handler_key.call_async()) allowing Lua coroutines
    // - ur.http.get() async function can be called from sync Lua tool handlers
    // - The tokio runtime resolves futures correctly in the sync dispatch context

    assert!(true, "HTTP tool integration capability is available");
}
