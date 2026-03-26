-- read-file extension: exposes a read_file tool to the LLM.
local ur = require("ur")

local MAX_BYTES = 128 * 1024 -- 128 KB

local function read_content(path)
    local ok, content = pcall(ur.fs.read, path)

    if not ok then
        return nil, tostring(content)
    end

    return content
end

local function split_lines(content)
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

    return lines
end

local function slice_lines(lines, offset, limit)
    local start = 1
    if offset and offset > 1 then
        start = offset
    end

    if start > #lines then
        return {}
    end

    local finish = #lines
    if limit and limit > 0 then
        finish = math.min(start + limit - 1, #lines)
    end

    local sliced = {}
    for i = start, finish do
        sliced[#sliced + 1] = lines[i]
    end

    return sliced
end

local function truncate_result(lines)
    local result = table.concat(lines)
    if #result <= MAX_BYTES then
        return result
    end

    local full_size = #result
    local truncated = {}
    local size = 0
    for _, line in ipairs(lines) do
        if size + #line > MAX_BYTES then
            break
        end
        truncated[#truncated + 1] = line
        size = size + #line
    end

    local truncated_result = table.concat(truncated)
    return truncated_result
        .. "\n[truncated — content was "
        .. full_size
        .. " bytes, returned first "
        .. size
        .. " bytes. Use offset/limit to read in chunks.]"
end

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

        local content, err = read_content(path)
        if not content then
            return { error = err }
        end

        local lines = split_lines(content)
        local sliced = slice_lines(lines, args.offset, args.limit)
        return { content = truncate_result(sliced) }
    end,
})

ur.log("read-file extension loaded")
