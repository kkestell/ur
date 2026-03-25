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

-- Register all 9 lifecycle hooks.
-- Hooks exercise mutation where applicable to validate that the host
-- applies returned modifications.

ur.hook("before_completion", function(ctx)
    ur.log("hook: before_completion, model=" .. tostring(ctx.model))
    -- Pass through without mutation (could modify model here).
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
    -- Could modify ctx.result to alter tool output.
    return { action = "pass" }
end)

ur.hook("before_session_load", function(ctx)
    ur.log("hook: before_session_load, session=" .. tostring(ctx.session_id))
    return { action = "pass" }
end)

ur.hook("after_session_load", function(ctx)
    ur.log("hook: after_session_load, events=" .. tostring(ctx.event_count))
    return { action = "pass" }
end)

ur.hook("before_session_append", function(ctx)
    ur.log("hook: before_session_append")
    return { action = "pass" }
end)

ur.hook("before_compaction", function(ctx)
    ur.log("hook: before_compaction, messages=" .. tostring(ctx.message_count))
    return { action = "pass" }
end)

ur.hook("after_compaction", function(ctx)
    ur.log("hook: after_compaction")
    return { action = "pass" }
end)

ur.log("test-extension loaded: 1 tool, 9 hooks")
