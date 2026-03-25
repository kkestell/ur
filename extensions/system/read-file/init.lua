-- read-file extension: exposes a read_file tool to the LLM.
local ur = require("ur")

local MAX_BYTES = 128 * 1024 -- 128 KB

ur.tool("read_file", {
    description = "Read the contents of a file. Supports optional line-based offset and limit for partial reads.",
    parameters = {
        type = "object",
        properties = {
            path = { type = "string", description = "Absolute path to the file to read" },
            offset = { type = "integer", description = "1-based line number to start reading from (default: 1)" },
            limit = { type = "integer", description = "Maximum number of lines to return (default: all remaining)" },
        },
        required = { "path" },
    },
    handler = function(args)
        local path = args.path
        if not path or path == "" then
            return { error = "path is required" }
        end

        local ok, content = pcall(function()
            return ur.fs.read(path)
        end)

        if not ok then
            return { error = tostring(content) }
        end

        -- Split into lines, preserving line endings for faithful reconstruction.
        local lines = {}
        local pos = 1
        while pos <= #content do
            local nl = content:find("\n", pos, true)
            if nl then
                lines[#lines + 1] = content:sub(pos, nl)
                pos = nl + 1
            else
                lines[#lines + 1] = content:sub(pos)
                pos = #content + 1
            end
        end

        -- Apply offset (1-based).
        local start = 1
        if args.offset and args.offset > 1 then
            start = args.offset
        end

        -- Apply limit.
        local finish = #lines
        if args.limit and args.limit > 0 then
            finish = math.min(start + args.limit - 1, #lines)
        end

        -- Clamp start.
        if start > #lines then
            return { content = "" }
        end

        -- Reassemble selected lines.
        local sliced = {}
        for i = start, finish do
            sliced[#sliced + 1] = lines[i]
        end
        local result = table.concat(sliced)

        -- Truncation guard.
        if #result > MAX_BYTES then
            -- Truncate at a line boundary within MAX_BYTES.
            local full_size = #result
            local truncated = {}
            local size = 0
            for _, line in ipairs(sliced) do
                if size + #line > MAX_BYTES then
                    break
                end
                truncated[#truncated + 1] = line
                size = size + #line
            end
            result = table.concat(truncated)
            result = result .. "\n[truncated — content was " .. full_size .. " bytes, returned first " .. #result .. " bytes. Use offset/limit to read in chunks.]"
        end

        return { content = result }
    end,
})

ur.log("read-file extension loaded")
