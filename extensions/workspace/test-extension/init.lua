-- Test extension: exercises tools and all 9 lifecycle hooks.

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
            -- Return the table as-is (will be serialized to JSON).
            return args
        end
        return tostring(args)
    end,
})

-- Register all 9 lifecycle hooks with observable side effects (logging).
ur.hook("before_completion", function(ctx)
    ur.log("hook: before_completion")
    return { action = "pass" }
end)

ur.hook("after_completion", function(ctx)
    ur.log("hook: after_completion")
    return { action = "pass" }
end)

ur.hook("before_tool", function(ctx)
    ur.log("hook: before_tool for " .. (ctx.tool_name or "unknown"))
    return { action = "pass" }
end)

ur.hook("after_tool", function(ctx)
    ur.log("hook: after_tool for " .. (ctx.tool_name or "unknown"))
    return { action = "pass" }
end)

ur.hook("before_session_load", function(ctx)
    ur.log("hook: before_session_load")
    return { action = "pass" }
end)

ur.hook("after_session_load", function(ctx)
    ur.log("hook: after_session_load")
    return { action = "pass" }
end)

ur.hook("before_session_append", function(ctx)
    ur.log("hook: before_session_append")
    return { action = "pass" }
end)

ur.hook("before_compaction", function(ctx)
    ur.log("hook: before_compaction")
    return { action = "pass" }
end)

ur.hook("after_compaction", function(ctx)
    ur.log("hook: after_compaction")
    return { action = "pass" }
end)

ur.log("test-extension loaded: 1 tool, 9 hooks")
