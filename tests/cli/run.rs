//! Run command integration tests.
//!
//! Tests for the turn execution and session management.
//!
//! ## HTTP tool integration test
//!
//! The test extension includes an `http_status` tool demonstrating async HTTP capability.
//! The tool is implemented in Lua and calls `ur.http.get()` to fetch a URL and return
//! the HTTP status code.
//!
//! ## Proving async capability
//!
//! This capability demonstrates that:
//! - `main()` is wrapped with `#[tokio::main]` providing a runtime
//! - `call_tool()` uses `block_in_place(block_on(...))` allowing Lua coroutines
//! - `ur.http.get()` can be called from sync Lua tool handlers
//! - The tokio runtime resolves futures correctly in sync dispatch context
