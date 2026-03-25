# Lifecycle Hooks

Hooks let extensions observe and mutate the agent lifecycle. There are nine hook points, each called with a context table and expecting a return table with an `action` field.

## Dispatch

Hooks are called across all enabled extensions in order: system tier first, then user, then workspace. Each extension sees the (possibly modified) context from the previous extension.

Hook ordering within a tier can be overridden in the workspace manifest.

## Return Values

Every hook must return a table with an `action` field:

```lua
{ action = "pass" }
```

No changes. Continue the chain.

```lua
{ action = "modify", key = value, ... }
```

Merge the returned key-value pairs into the context (excluding `action`), then continue the chain. Only include keys you want to change — unmentioned keys are preserved.

```lua
{ action = "reject", reason = "..." }
```

Stop the chain and reject the operation. Only `before_*` hooks can reject. If an `after_*` hook returns `reject`, it is ignored and treated as `pass`.

If a hook handler throws an error, it is logged and skipped. The chain continues with the next extension.

---

## Completion Hooks

### before_completion

Called before each LLM completion request. Can modify the request or reject it entirely.

**Context fields:**

| Field | Type | Description |
|-------|------|-------------|
| `messages` | list of Message | Conversation history being sent to the LLM |
| `model` | string | Model ID for this completion |
| `settings` | list of `{key, value}` | Provider settings (temperature, etc.) |
| `tools` | list of ToolDescriptor | Tools available to the LLM |

**Mutable fields:** `messages`, `model`, `settings`, `tools`

**Example — force a specific model:**

```lua
ur.hook("before_completion", function(ctx)
    return { action = "modify", model = "gemini-2.0-flash" }
end)
```

**Example — reject completions that include too many messages:**

```lua
ur.hook("before_completion", function(ctx)
    if #ctx.messages > 200 then
        return { action = "reject", reason = "message history too long" }
    end
    return { action = "pass" }
end)
```

### after_completion

Called after the LLM returns a completion. Can modify the response.

**Context fields:**

| Field | Type | Description |
|-------|------|-------------|
| `messages` | list of Message | Conversation history that was sent |
| `model` | string | Model ID that was used |
| `response` | Message | The LLM's response message |

**Mutable fields:** `response`

**Example — log token usage:**

```lua
ur.hook("after_completion", function(ctx)
    ur.log("completion from " .. tostring(ctx.model))
    return { action = "pass" }
end)
```

---

## Tool Hooks

### before_tool

Called before each tool invocation. Can modify the arguments or reject the call.

**Context fields:**

| Field | Type | Description |
|-------|------|-------------|
| `tool_name` | string | Name of the tool being called |
| `arguments` | string | JSON-encoded tool arguments |
| `call_id` | string | Unique ID for this tool call |

**Mutable fields:** `arguments`

**Example — block a dangerous tool:**

```lua
ur.hook("before_tool", function(ctx)
    if ctx.tool_name == "rm" then
        return { action = "reject", reason = "rm is not allowed" }
    end
    return { action = "pass" }
end)
```

**Example — inject a default argument:**

```lua
ur.hook("before_tool", function(ctx)
    if ctx.tool_name == "search" then
        -- Parse, modify, re-serialize
        -- (arguments is a JSON string)
        local args = ctx.arguments
        if not string.find(args, '"limit"') then
            args = string.gsub(args, '}$', ',"limit":10}')
            return { action = "modify", arguments = args }
        end
    end
    return { action = "pass" }
end)
```

### after_tool

Called after a tool returns its result. Can modify the result string.

**Context fields:**

| Field | Type | Description |
|-------|------|-------------|
| `tool_name` | string | Name of the tool that was called |
| `call_id` | string | Unique ID for this tool call |
| `result` | string | The tool's result content |

**Mutable fields:** `result`

**Example — redact secrets from tool output:**

```lua
ur.hook("after_tool", function(ctx)
    local result = ctx.result or ""
    if string.find(result, "SECRET") then
        return { action = "modify", result = "[REDACTED]" }
    end
    return { action = "pass" }
end)
```

---

## Session Hooks

### before_session_load

Called before loading a session from storage. Can reject the load.

**Context fields:**

| Field | Type | Description |
|-------|------|-------------|
| `session_id` | string | ID of the session being loaded |

**Mutable fields:** `session_id`

**Example — block loading specific sessions:**

```lua
ur.hook("before_session_load", function(ctx)
    if ctx.session_id == "locked" then
        return { action = "reject", reason = "session is locked" }
    end
    return { action = "pass" }
end)
```

### after_session_load

Called after a session's messages are loaded from storage. Can modify the loaded messages.

**Context fields:**

| Field | Type | Description |
|-------|------|-------------|
| `session_id` | string | ID of the session that was loaded |
| `messages` | list of Message | Messages reconstructed from session events |

**Mutable fields:** `messages`

**Example — log session size:**

```lua
ur.hook("after_session_load", function(ctx)
    ur.log("loaded session " .. ctx.session_id .. " with " .. #ctx.messages .. " messages")
    return { action = "pass" }
end)
```

### before_session_append

Called before each event is persisted to session storage. Can modify the event or reject it (skipping persistence for that event).

**Context fields:**

| Field | Type | Description |
|-------|------|-------------|
| `session_id` | string | ID of the current session |
| `event` | SessionEvent | The event about to be persisted |

**Mutable fields:** `event`

**Example — skip persisting tool approval events:**

```lua
ur.hook("before_session_append", function(ctx)
    if ctx.event and ctx.event.type == "tool_approval_requested" then
        return { action = "reject", reason = "don't persist approval events" }
    end
    return { action = "pass" }
end)
```

---

## Compaction Hooks

### before_compaction

Called before the message history is compacted. Can modify the messages or reject compaction entirely.

**Context fields:**

| Field | Type | Description |
|-------|------|-------------|
| `messages` | list of Message | The full message history to be compacted |

**Mutable fields:** `messages`

**Example — skip compaction when history is short:**

```lua
ur.hook("before_compaction", function(ctx)
    if #ctx.messages < 20 then
        return { action = "reject", reason = "not enough messages to compact" }
    end
    return { action = "pass" }
end)
```

### after_compaction

Called after compaction produces a new (shorter) message list. Can modify the compacted result.

**Context fields:**

| Field | Type | Description |
|-------|------|-------------|
| `original` | list of Message | The messages before compaction |
| `compacted` | list of Message | The messages after compaction |

**Mutable fields:** `compacted`

**Example — log compaction ratio:**

```lua
ur.hook("after_compaction", function(ctx)
    local before = #(ctx.original or {})
    local after = #(ctx.compacted or {})
    ur.log("compaction: " .. before .. " -> " .. after .. " messages")
    return { action = "pass" }
end)
```
