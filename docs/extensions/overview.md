# Extension Management

ur discovers and loads extensions automatically from three tiers of directories. This guide explains how extension discovery works, how to enable and disable extensions, and how slots organize extensions by capability.

## Discovery Tiers

ur scans for extensions in three locations, in order. The system and user tiers live under `UR_ROOT`, which defaults to `~/.ur` when the environment variable is unset.

| Tier | Location | Purpose |
|------|----------|---------|
| **System** | `$UR_ROOT/extensions/system/` | Bundled extensions shipped with ur |
| **User** | `$UR_ROOT/extensions/user/` | Extensions you install for all workspaces |
| **Workspace** | `.ur/extensions/` | Extensions specific to a single project |

Each extension is a subdirectory containing a `.wasm` file. When you run ur, it:

1. Scans all three tiers for extension directories
2. Loads each `.wasm` component to query its identity and capabilities
3. Merges results with the workspace manifest
4. Validates that required slots are satisfied

### Extension Directories

An extension directory must contain exactly one `.wasm` file, either at the directory root or in a subdirectory. Discovery searches the entire extension directory tree and errors if multiple `.wasm` files are found for one extension.

## The Workspace Manifest

ur stores workspace state in a manifest file at `$UR_ROOT/workspaces/<workspace-path>/manifest.json`, where `<workspace-path>` is the workspace directory path with slashes replaced by underscores. For example, with the default `UR_ROOT` and a workspace at `/home/user/projects/myapp`, the manifest path would be `~/.ur/workspaces/home_user_projects_myapp/manifest.json`.

The manifest tracks:

- Which extensions were discovered
- Whether each extension is enabled or disabled
- The checksum of each extension's WASM file

The manifest persists your preferences across sessions. When ur starts, it:

1. Discovers all available extensions
2. Loads the existing manifest (if any)
3. Merges discovered extensions with saved preferences
4. Saves the updated manifest

### Default State

New extensions default to enabled or disabled based on their tier:

- **System extensions** — Enabled by default
- **User extensions** — Disabled by default
- **Workspace extensions** — Disabled by default

This ensures bundled extensions work out of the box while requiring explicit opt-in for custom extensions.

## Enabling and Disabling Extensions

Extensions can be enabled or disabled through ur's commands. The behavior depends on the extension's slot.

### Enable

When you enable an extension:

1. ur checks if the extension is already enabled
2. For slots with "exactly one" cardinality, the current occupant is automatically disabled
3. The extension is marked as enabled in the manifest

### Disable

When you disable an extension:

1. ur checks if the extension is already disabled
2. If the extension fills a required slot and is the only provider, disable fails
3. Otherwise, the extension is marked as disabled in the manifest

You cannot disable the last provider of a required slot — you must enable a replacement first.

## Slots

Extensions fill *slots* — named capability points in ur's architecture. A slot defines what kind of service an extension provides.

### Available Slots

| Slot | Purpose | Cardinality | Required |
|------|---------|-------------|----------|
| `llm-provider` | LLM completions | At least one | Yes |
| `session-provider` | Conversation persistence | Exactly one | Yes |
| `compaction-provider` | Message summarization | Exactly one | Yes |
| (none) | General-purpose tools | Unlimited | No |

### Cardinality Rules

Cardinality determines how many extensions can fill a slot simultaneously:

**At least one** — Multiple providers can coexist. ur resolves a role to a configured `provider/model` pair and selects the matching provider. Example: multiple `llm-provider` extensions can stay enabled at once, while role resolution chooses which one handles a given request.

**Exactly one** — Only one provider active at a time. Enabling a new provider automatically disables the current one (switch behavior). Example: `session-provider` ensures a single source of truth for conversation history.

**Unlimited** — Any number of extensions with no slot. These are general-purpose extensions that don't provide core infrastructure.

### Required Slots

Some slots are *required* — ur cannot start without at least one enabled provider. The `session-provider` and `compaction-provider` slots are required with "exactly one" cardinality, meaning exactly one extension must fill each.

If you try to disable the only provider of a required slot, ur will refuse and explain why.

## Extension Identity

Each extension declares its identity through two functions:

- **ID** — A unique identifier like `llm-google` or `session-jsonl`. This must be unique across all discovered extensions.
- **Name** — A human-readable name like "Google Gemini" or "JSONL Sessions" displayed in UI and logs.

Duplicate IDs across tiers are an error — even if one extension is disabled, no two extensions can share an ID.

## Checksums

The manifest stores a SHA-256 checksum of each extension's WASM file. This allows ur to detect when an extension has been rebuilt or modified. When an extension's checksum changes:

- The manifest updates with the new checksum
- The extension's enabled state is preserved

## Summary

- Extensions are discovered automatically from system, user, and workspace tiers
- The manifest persists enabled/disabled state per workspace
- Slots organize extensions by capability with cardinality rules
- Required slots must have at least one enabled provider
- "Exactly one" slots behave like switches — enabling one disables the other
