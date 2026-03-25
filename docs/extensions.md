# Extensions

Extensions add tools and hooks to ur.

## Installing an Extension

An extension is a directory containing an `extension.toml` file and an `init.lua` script. To install one, copy or clone its directory into one of the extension locations below.

## Extension Locations

ur scans three directories for extensions, in order. The system and user directories live under `UR_ROOT`, which defaults to `~/.ur`.

| Location | Purpose |
|----------|---------|
| `$UR_ROOT/extensions/system/` | Bundled with ur |
| `$UR_ROOT/extensions/user/` | Installed for all workspaces |
| `.ur/extensions/` | Specific to a single project |

Use the **user** directory for extensions you want everywhere. Use the **workspace** directory (`.ur/extensions/` in your project root) for project-specific extensions.

## Enabling and Disabling

Extensions default to enabled or disabled based on where they are installed:

- **System** extensions are enabled by default
- **User** extensions are disabled by default
- **Workspace** extensions are disabled by default


## Capabilities

Extensions run sandboxed with no filesystem or network access by default. Each extension declares the permissions it needs in its `extension.toml`:

| Capability | What it grants |
|------------|---------------|
| `network` | Outbound HTTP requests |
| `fs-read` | Read files and list directories |
| `fs-write` | Write files |

An extension with no declared capabilities can still register tools and hooks — it just can't reach outside the sandbox.

## Checking Installed Extensions

Each extension has an `extension.toml` with its ID, name, and capabilities:

```toml
[extension]
id = "tool-logger"
name = "Tool Call Logger"
capabilities = ["network"]
```

Review an extension's `extension.toml` and `init.lua` before enabling it to understand what permissions it requests and what it does.
