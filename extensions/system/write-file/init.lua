-- write-file extension: exposes a write_file tool to the LLM.
local ur = require("ur")

ur.tool("write_file", {
    description = "Write content to a file, creating it if it does not exist or overwriting if it does.",
    parameters = {
        type = "object",
        properties = {
            path = { type = "string", description = "Absolute path to the file to write" },
            content = { type = "string", description = "Content to write to the file" },
        },
        required = { "path", "content" },
    },
    handler = function(args)
        local path = args.path
        if not path or path == "" then
            return { error = "path is required" }
        end

        local content = args.content
        if content == nil then
            return { error = "content is required" }
        end

        local ok, err = pcall(ur.fs.write, path, content)
        if not ok then
            return { error = tostring(err) }
        end

        return { written = #content, path = path }
    end,
})

ur.log("write-file extension loaded")
