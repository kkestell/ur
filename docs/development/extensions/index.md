# Developing Extensions

## Creating an Extension

An extension is a directory with two files:

```
my-extension/
  extension.toml
  init.lua
```

Place the directory in one of the three discovery locations:

| Location | Default State |
|----------|---------------|
| `$UR_ROOT/extensions/system/` | Enabled |
| `$UR_ROOT/extensions/user/` | Disabled |
| `.ur/extensions/` | Disabled |

`UR_ROOT` defaults to `~/.ur`.

### extension.toml

The manifest declares the extension's identity and requested capabilities:

```toml
[extension]
id = "my-extension"
name = "My Extension"
capabilities = ["network"]
```

**Fields:**

- `id` (required) — Unique identifier across all locations. Duplicate IDs are a hard error.
- `name` (optional) — Human-readable name for UI and logs. Defaults to `id`.
- `capabilities` (optional) — List of capability grants. Omit for a zero-permission extension.

**Available capabilities:**

| Capability | What it unlocks |
|------------|----------------|
| `network` | `ur.http.get()`, `ur.http.post()` |
| `fs-read` | `ur.fs.read()`, `ur.fs.list()` |
| `fs-write` | `ur.fs.write()` |

### init.lua

The entry point. It runs once when ur loads the extension. Use it to register tools and hooks:

```lua
local ur = require("ur")

ur.log("my-extension loaded")
```

## Registering Tools

Tools are functions the LLM can call. Register them with `ur.tool(name, spec)`:

```lua
ur.tool("word_count", {
    description = "Count words in a string",
    parameters = {
        type = "object",
        properties = {
            text = { type = "string", description = "The text to count" },
        },
    },
    handler = function(args)
        local count = 0
        for _ in (args.text or ""):gmatch("%S+") do
            count = count + 1
        end
        return { count = count }
    end,
})
```

**spec fields:**

- `description` — Shown to the LLM so it knows when to use the tool.
- `parameters` — JSON Schema object describing the tool's input.
- `handler` — The function called when the LLM invokes the tool. Receives parsed arguments as a table. Return a string or table (tables are serialized to JSON).

Tool handlers support async — if you call `ur.http.get()` or other async host APIs inside a handler, they execute as coroutines without blocking the runtime.

## Registering Hooks

Hooks let extensions observe and mutate the agent lifecycle. Register them with `ur.hook(name, fn)`:

```lua
ur.hook("before_completion", function(ctx)
    ur.log("model: " .. tostring(ctx.model))
    return { action = "pass" }
end)
```

See [hooks.md](hooks.md) for the full hook reference with context fields and examples.

## The `ur` Host API

### Always Available

| Function | Description |
|----------|-------------|
| `ur.log(msg)` | Log a message through the host |
| `ur.config` | Extension configuration table from user config |
| `ur.tool(name, spec)` | Register a tool |
| `ur.hook(name, fn)` | Register a lifecycle hook |
| `ur.complete(messages, opts)` | Call the LLM directly (bypasses hooks) |
| `ur.session.load(id)` | Load a session's events by ID |
| `ur.session.list()` | List available sessions |

#### ur.complete(messages, opts)

Calls the LLM directly, bypassing hooks.

```lua
local result = ur.complete({
    { role = "user", parts = {{ type = "text", text = "Summarize this" }} },
}, { provider = "google", model = "gemini-2.0-flash" })
-- result is a string containing the model's text response
```

`opts` is optional. Fields:
- `provider` — Provider ID to use. Defaults to the first available.
- `model` — Model ID. Defaults to the provider's default model.

### Capability-Gated

#### ur.http (requires `network`)

```lua
local response = ur.http.get("https://example.com", {
    headers = { ["Authorization"] = "Bearer ..." },
})
-- response.status: number (HTTP status code)
-- response.body: string (response body)

local response = ur.http.post("https://example.com/api", '{"key":"value"}', {
    headers = { ["Content-Type"] = "application/json" },
})
```

Both return `{ status = number, body = string }`. These are async — they yield to the runtime and resume when the response arrives.

#### ur.fs (requires `fs-read` and/or `fs-write`)

```lua
-- fs-read
local content = ur.fs.read("/path/to/file")
local entries = ur.fs.list("/path/to/dir")  -- returns list of filenames

-- fs-write
ur.fs.write("/path/to/file", "content")
```

## Sandbox Constraints

Each extension runs in an isolated Luau VM. The sandbox imposes several limits:

- **No shared state** — Extensions cannot access each other's VMs.
- **Restricted stdlib** — Sandbox mode disables `os`, `io`, `debug`, `dofile`, `loadfile`, and other dangerous globals. Only `require("ur")` is available; all other modules are blocked.
- **Memory limit** — 64 MB per VM. Exceeding this terminates the extension.
- **Instruction budget** — The host checks for runaway execution every 100K instructions and terminates extensions that exceed the budget.
- **No ambient capabilities** — Filesystem and network access require explicit capability grants in `extension.toml`. Without them, the corresponding `ur.fs` and `ur.http` sub-modules are not injected.

## Error Handling

Errors in tool handlers are caught and returned to the LLM as `"Error: ..."` strings. The agent loop continues.

Errors in hook handlers are logged and the hook is skipped — the chain continues with the next extension. A failing hook never crashes the session.

Use `pcall` for operations that may fail:

```lua
local ok, result = pcall(function()
    return ur.http.get("https://unreliable-api.example.com")
end)
if not ok then
    ur.log("request failed: " .. tostring(result))
    return { status = 0, error = tostring(result) }
end
```

## Complete Example

An extension that logs every tool call to an HTTP endpoint:

```toml
# extension.toml
[extension]
id = "tool-logger"
name = "Tool Call Logger"
capabilities = ["network"]
```

```lua
-- init.lua
local ur = require("ur")
local endpoint = ur.config.endpoint or "https://logs.example.com/tools"

ur.hook("after_tool", function(ctx)
    local ok, err = pcall(function()
        ur.http.post(endpoint, '{"tool":"' .. ctx.tool_name .. '","call_id":"' .. ctx.call_id .. '"}', {
            headers = { ["Content-Type"] = "application/json" },
        })
    end)
    if not ok then
        ur.log("tool-logger: failed to send log: " .. tostring(err))
    end
    return { action = "pass" }
end)

ur.log("tool-logger loaded, endpoint=" .. endpoint)
```
