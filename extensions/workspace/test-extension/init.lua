-- Test extension: exercises tools, require("ur"), config, and all 9 lifecycle hooks.
local ur = require("ur")

-- Verify config is accessible (may be empty table in tests).
ur.log("test-extension config: " .. tostring(ur.config))

-- Register the echo tool: returns its input as-is.
ur.tool("echo", {
    description = "Echoes back the input arguments as JSON",
    parameters = {
        type = "object",
        properties = {
            message = { type = "string", description = "The message to echo" },
        },
    },
    handler = function(args)
        ur.log("echo tool called")
        if type(args) == "table" then
            return args
        end
        return tostring(args)
    end,
})

-- Register the http_status tool: calls ur.http.get and returns the status code.
ur.tool("http_status", {
    description = "Fetch a URL and return the HTTP status code",
    parameters = {
        type = "object",
        properties = {
            url = { type = "string", description = "The URL to fetch" },
        },
    },
    handler = function(args)
        ur.log("http_status tool called with url=" .. tostring(args.url or ""))
        local url = args.url
        if not url then
            return { status = 0, error = "no url provided" }
        end

        local success, response = pcall(function()
            return ur.http.get(url)
        end)

        if not success then
            ur.log("http_status: request failed: " .. tostring(response))
            return { status = 0, error = tostring(response) }
        end

        ur.log("http_status: request succeeded, status=" .. tostring(response.status))
        return { status = response.status, content_length = #(response.content or "") }
    end,
})

-- Register all 9 lifecycle hooks.
-- Hooks exercise mutation where applicable to validate that the host
-- applies returned modifications.

ur.hook("before_completion", function(ctx)
    ur.log("hook: before_completion, model=" .. tostring(ctx.model))
    -- Demonstrate mutation: prepend a marker to the model ID if it doesn't already have one
    local model = ctx.model or ""
    if not string.find(model, "test-extension-marked") then
        local modified_model = "test-extension-marked:" .. model
        ur.log("hook: before_completion modifying model to " .. modified_model)
        return { action = "modify", model = modified_model }
    end
    return { action = "pass" }
end)

ur.hook("after_completion", function(ctx)
    ur.log("hook: after_completion")
    return { action = "pass" }
end)

ur.hook("before_tool", function(ctx)
    ur.log("hook: before_tool for " .. tostring(ctx.tool_name))
    -- Pass through; could modify ctx.arguments to alter tool input.
    return { action = "pass" }
end)

ur.hook("after_tool", function(ctx)
    ur.log("hook: after_tool for " .. tostring(ctx.tool_name))
    -- Demonstrate mutation: append a suffix to the tool result
    local result = ctx.result or ""
    local modified_result = result .. " [modified by test-extension hook]"
    ur.log("hook: after_tool modifying result to: " .. modified_result)
    return { action = "modify", result = modified_result }
end)

ur.hook("before_session_load", function(ctx)
    ur.log("hook: before_session_load, session=" .. tostring(ctx.session_id))
    return { action = "pass" }
end)

ur.hook("after_session_load", function(ctx)
    ur.log("hook: after_session_load, messages=" .. tostring(#(ctx.messages or {})))
    -- Could modify messages here; currently just observing
    return { action = "pass" }
end)

ur.hook("before_session_append", function(ctx)
    local event_type = "unknown"
    if ctx.event and ctx.event.type then
        event_type = ctx.event.type
    end
    ur.log("hook: before_session_append, event_type=" .. event_type)
    -- Could modify the event here; currently just observing
    return { action = "pass" }
end)

ur.hook("before_compaction", function(ctx)
    ur.log("hook: before_compaction, messages=" .. tostring(#(ctx.messages or {})))
    -- Could modify messages here; currently just observing
    return { action = "pass" }
end)

ur.hook("after_compaction", function(ctx)
    ur.log("hook: after_compaction, original=" .. tostring(#(ctx.original or {})) .. ", compacted=" .. tostring(#(ctx.compacted or {})))
    -- Could modify compacted messages here; currently just observing
    return { action = "pass" }
end)

ur.log("test-extension loaded: 1 tool, 9 hooks")
