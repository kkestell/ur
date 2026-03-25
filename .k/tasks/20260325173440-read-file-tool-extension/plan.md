# Add `read_file` Extension

## Goal

Ship a first-party `read_file` Lua extension that exposes a `read_file` tool to the LLM, allowing it to read file contents during a session.

## Desired outcome

The LLM can call `read_file` with a file path and receive the file's contents. The extension lives in `extensions/system/read-file/` as a first-party system extension (enabled by default on discovery) and is exercised by integration tests.

## Related code

- `extensions/workspace/test-extension/` — Existing extension; template for directory structure, manifest, and Lua patterns
- `src/manifest.rs:99-118` — System extensions default to enabled; user/workspace default to disabled
- `src/host_api.rs` — Defines `ur.fs.read(path)` (the host API the extension will call), gated on `fs-read` capability
- `src/lua_host.rs` — `LuaExtension::load()`, `call_tool()`, `RegisteredTool`
- `src/discovery.rs` — Three-tier discovery; scans for `extension.toml` in extension directories
- `src/types.rs` — `ExtensionCapabilities::from_strings` parses `"fs-read"` capability string
- `src/workspace.rs` — Loads enabled extensions, wires tool handlers into sessions
- `tests/cli/extension.rs` — Integration test patterns: `install_test_extension`, CLI assertions
- `tests/cli/run.rs:151` — `test_extension_http_status_tool` — end-to-end tool dispatch test pattern

## Current state

- `ur.fs.read(path)` already exists in the host API, gated behind the `fs-read` capability. It returns the full file contents as a string.
- `ur.fs.list(path)` also exists for directory listing.
- No extension currently wraps these as LLM-callable tools.
- The test extension demonstrates the complete tool registration pattern (`ur.tool(name, spec)` with `description`, `parameters`, and `handler`).

## Approach

Create a Lua extension that:
1. Declares `fs-read` capability in `extension.toml`
2. Registers a `read_file` tool via `ur.tool()` that calls `ur.fs.read(path)` and returns the content
3. Supports optional `offset` and `limit` parameters for partial file reads (line-based)
4. Enforces a max payload size — truncates if exceeded and appends a message noting truncation and full payload size
5. Include integration tests following the patterns in `tests/cli/extension.rs` and `tests/cli/run.rs`

### Tool parameters

- `path` (string, required) — file to read
- `offset` (integer, optional) — 1-based line number to start reading from (default: 1)
- `limit` (integer, optional) — max number of lines to return (default: all remaining lines)

### Truncation guard

After slicing by offset/limit, if the resulting content exceeds a max byte threshold (e.g. 128 KB), truncate to that limit at a line boundary and append:

```
[truncated — content was {full_size} bytes, returned first {returned_size} bytes. Use offset/limit to read in chunks.]
```

This prevents blowing up the context window with a single tool call. Error handling uses `pcall` around `ur.fs.read` to return structured error messages rather than crashing.

## Implementation plan

- [x] Create `extensions/system/read-file/extension.toml` with id `read-file`, name `Read File`, capability `fs-read`
- [x] Create `extensions/system/read-file/init.lua` that registers a `read_file` tool:
  - Parameters: `path` (string, required), `offset` (integer, optional), `limit` (integer, optional)
  - Handler reads full file via `ur.fs.read(args.path)` wrapped in `pcall`
  - Splits into lines, applies offset/limit slicing
  - Checks byte size of result against max threshold; truncates at line boundary if exceeded
  - On truncation, appends message with full vs returned byte counts and a hint to use offset/limit
  - Returns `{ content = "..." }` on success, `{ error = "..." }` on failure
- [x] Add integration tests in `tests/extensions/` and CLI tests in `tests/cli/extension.rs`:
  - Test that the system extension is discovered, listed, and enabled by default
  - Test that inspecting it shows the `read_file` tool
  - Test end-to-end tool dispatch: create a temp file, call `read_file` tool, verify content is returned
  - Test offset/limit: write a multi-line file, read with offset and limit, verify correct line range
  - Test truncation: write a file exceeding the max threshold, verify output is truncated and includes the truncation message
- [x] Run `make verify` to confirm everything passes

## Validation

- `make verify` passes (fmt, check, test, clippy)
- Integration test proves `read_file` tool returns correct file contents
- Integration test proves offset/limit slicing works correctly
- Integration test proves truncation fires and includes the expected message
- Extension appears in `ur extension list` and `ur extension inspect read-file` output
