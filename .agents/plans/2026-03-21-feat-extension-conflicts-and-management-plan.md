---
title: "Extension Conflicts and Management"
type: feat
date: 2026-03-21
---

# Extension Conflicts and Management

## Overview

Replace the single test-extension with a three-tier extension discovery system, host-declared slots with cardinality enforcement, manifest-based state management, and CLI commands for listing, enabling, disabling, and inspecting extensions.

## Problem Statement / Motivation

The current extension system loads a single hardcoded `.wasm` file. To move toward "everything is an extension," we need:

- Multiple extensions discovered from known directories
- Conflict-free loading via typed slots with cardinality constraints
- Persistent enabled/disabled state via workspace manifests
- CLI management commands

Implements decisions from the [extension conflicts brainstorm](../brainstorms/2026-03-21-extension-conflicts-and-management-brainstorm.md).

## Proposed Solution

### Directory Layout

```
extensions/
├── system/
│   ├── session-jsonl/          # fills session-provider (exactly-1)
│   ├── compaction-llm/         # fills compaction-provider (exactly-1)
│   └── llm-openai/             # fills llm-provider (at-least-1)
├── user/
│   └── llm-anthropic/          # fills llm-provider (at-least-1)
workspace/
└── .ur/
    └── extensions/
        └── test-extension/     # no slot
```

> **Note:** The user-tier extension is `llm-anthropic` (not a second `llm-openai`) to avoid ID collision while still testing llm-provider cardinality.

### Discovery Paths

| Tier | Path | Default State |
|------|------|---------------|
| System | `$UR_ROOT/extensions/system/` | Enabled |
| User | `$UR_ROOT/extensions/user/` | Disabled |
| Workspace | `$WORKSPACE/.ur/extensions/` | Disabled |

`UR_ROOT` env var, defaults to `~/.ur`. For testing: `UR_ROOT=/home/kyle/src/ur`.

Discovery scans each tier directory recursively for `*.wasm` files, loads each to call `register()`, and records the manifest metadata.

### Host-Declared Slots

| Slot | Cardinality | Required |
|------|------------|----------|
| `session-provider` | Exactly 1 | Yes |
| `compaction-provider` | Exactly 1 | Yes |
| `llm-provider` | At least 1 | Yes |

Enforcement rules:

- Cannot disable the sole provider of a required slot
- Enabling a second extension in an exactly-1 slot switches: disables the old, enables the new
- Unknown slot names in extension manifests are a hard error
- Duplicate extension IDs across tiers are a hard error

### Workspace Manifest

Stored at `$UR_ROOT/workspaces/<escaped-workspace-path>/manifest.json`.

```json
{
  "workspace": "/home/kyle/src/ur/workspace",
  "extensions": [
    {
      "id": "session-jsonl",
      "name": "Session JSONL",
      "slot": "session-provider",
      "source": "system",
      "wasm_path": "/absolute/path/to/session_jsonl.wasm",
      "checksum": "sha256:abc123...",
      "enabled": true
    }
  ]
}
```

Path escaping: canonical absolute workspace path with `/` replaced by `_`, leading `_` stripped. Example: `/home/kyle/src/ur/workspace` → `home_kyle_src_ur_workspace`.

### CLI

```
ur [-w <workspace>] extensions list
ur [-w <workspace>] extensions enable <id>
ur [-w <workspace>] extensions disable <id>
ur [-w <workspace>] extensions inspect <id>
```

Every command follows the same preamble:

1. Read `UR_ROOT` env (default `~/.ur`)
2. Resolve workspace path (from `-w`, or current directory)
3. Scan all three tiers for `.wasm` files
4. Load or create manifest, merge with discovered extensions
5. Write updated manifest
6. Execute the specific command

### `list` Output

```
ID               NAME              SLOT                 SOURCE     ENABLED
session-jsonl    Session JSONL     session-provider     system     ✓
compaction-llm   Compaction LLM    compaction-provider  system     ✓
llm-openai       LLM OpenAI       llm-provider         system     ✓
llm-anthropic    LLM Anthropic     llm-provider         user       ✗
test-extension   Test Extension    —                    workspace  ✗
```

## Technical Considerations

### Architecture

New modules:

| Module | Responsibility |
|--------|---------------|
| `src/cli.rs` | Clap-based argument parsing and command dispatch |
| `src/slot.rs` | Slot definitions and cardinality enforcement |
| `src/discovery.rs` | Three-tier `.wasm` scanning and registration |
| `src/manifest.rs` | Manifest serialization, persistence, and merge logic |

Modified modules:

| Module | Change |
|--------|--------|
| `src/main.rs` | Replace hardcoded demo with CLI dispatch |
| `src/extension_host.rs` | No major changes — reused for WASM loading |
| `wit/world.wit` | Add `slot` field to `extension-manifest` |
| `.gitignore` | Add `workspaces/`, change `/target` to `target/` for nested builds |

### New Host Dependencies

| Crate | Purpose |
|-------|---------|
| `clap` (derive feature) | CLI argument parsing |
| `serde` + `serde_json` (derive feature) | Manifest serialization |
| `sha2` | WASM file checksums |
| `walkdir` | Recursive directory scanning |

### WIT Change

```wit
record extension-manifest {
    id: string,
    name: string,
    slot: option<string>,
}
```

All extension stubs must use the updated manifest shape.

### Performance

Loading every `.wasm` on every CLI command is acceptable for a small number of extensions. If this becomes slow later, cache manifests by checksum and skip re-loading unchanged extensions.

### Engineering Quality

| Principle | Application |
|-----------|------------|
| **SRP** | Each new module has one job: slots, discovery, manifest, CLI |
| **OCP** | New slots added by extending the `SLOTS` array, not modifying logic |
| **YAGNI** | No tool/command collision detection yet (stubs register no tools). No inter-extension communication. No generic extension state. |

## Implementation Steps

### Step 1: WIT and Housekeeping

- [x] Add `slot: option<string>` to `extension-manifest` in [wit/world.wit](../../wit/world.wit)
- [x] Verify `extension_host.rs` compiles with the updated WIT
- [x] Update [.gitignore](../../.gitignore): change `/target` to `target/` (catches nested extension builds), add `workspaces/`
- [x] Delete `extensions/test-extension/`

**Files:** `wit/world.wit`, `src/extension_host.rs`, `.gitignore`

### Step 2: Create Extension Stubs

Create 5 minimal extension crates. Each is a `cdylib` targeting `wasm32-wasip2` with `wit-bindgen = "0.54"`. Each implements `register()` returning its id, name, and slot. `call_tool()` returns `Err("no tools implemented")` for all inputs.

- [x] `extensions/system/session-jsonl/` — id: `session-jsonl`, name: `Session JSONL`, slot: `Some("session-provider")`
- [x] `extensions/system/compaction-llm/` — id: `compaction-llm`, name: `Compaction LLM`, slot: `Some("compaction-provider")`
- [x] `extensions/system/llm-openai/` — id: `llm-openai`, name: `LLM OpenAI`, slot: `Some("llm-provider")`
- [x] `extensions/user/llm-anthropic/` — id: `llm-anthropic`, name: `LLM Anthropic`, slot: `Some("llm-provider")`
- [x] `workspace/.ur/extensions/test-extension/` — id: `test-extension`, name: `Test Extension`, slot: `None`

Each crate copies the lint configuration from the existing test-extension `Cargo.toml`.

WIT path (relative from each crate root):
- System extensions: `../../../wit`
- User extensions: `../../../wit`
- Workspace extension: `../../../../wit`

- [x] Build all 5: `cargo build --manifest-path <path>/Cargo.toml --target wasm32-wasip2 --release`

**Files:** 5× `Cargo.toml` + 5× `src/lib.rs`

### Step 3: Add Host Dependencies

- [x] `cargo add clap --features derive`
- [x] `cargo add serde --features derive`
- [x] `cargo add serde_json`
- [x] `cargo add sha2`
- [x] `cargo add walkdir`

### Step 4: Slot Definitions — `src/slot.rs`

- [x] Define `Cardinality` enum: `ExactlyOne`, `AtLeastOne`
- [x] Define `SlotDefinition` struct: `name: &'static str`, `cardinality: Cardinality`, `required: bool`
- [x] Define `SLOTS` constant with the three slots
- [x] `fn find_slot(name: &str) -> Option<&'static SlotDefinition>` — lookup by name
- [x] `fn validate_slot_name(name: &str) -> Result<()>` — errors on unknown slots

### Step 5: Discovery — `src/discovery.rs`

- [x] Define `SourceTier` enum: `System`, `User`, `Workspace` (with Display impl)
- [x] Define `DiscoveredExtension` struct: `id`, `name`, `slot: Option<String>`, `source: SourceTier`, `wasm_path: PathBuf`, `checksum: String`
- [x] `fn discover(engine: &Engine, ur_root: &Path, workspace: &Path) -> Result<Vec<DiscoveredExtension>>`
  - Walk `$UR_ROOT/extensions/system/` → tier System
  - Walk `$UR_ROOT/extensions/user/` → tier User
  - Walk `$WORKSPACE/.ur/extensions/` → tier Workspace (skip if dir missing)
  - For each `.wasm`: compute SHA-256, load via `ExtensionInstance::load()`, call `register()`, validate slot name
  - Validate no duplicate extension IDs
  - Return list
- [x] `fn compute_checksum(path: &Path) -> Result<String>` — `sha256:<hex>` format

### Step 6: Manifest Management — `src/manifest.rs`

- [x] Define `WorkspaceManifest` (serde Serialize/Deserialize): `workspace: String`, `extensions: Vec<ManifestEntry>`
- [x] Define `ManifestEntry` (serde): `id`, `name`, `slot: Option<String>`, `source: String`, `wasm_path: String`, `checksum: String`, `enabled: bool`
- [x] `fn manifest_dir(ur_root: &Path, workspace: &Path) -> PathBuf` — `$UR_ROOT/workspaces/<escaped>/`
- [x] `fn escape_workspace_path(path: &Path) -> String` — canonical path, `/` → `_`, strip leading `_`
- [x] `fn load_manifest(ur_root: &Path, workspace: &Path) -> Result<Option<WorkspaceManifest>>`
- [x] `fn save_manifest(ur_root: &Path, workspace: &Path, manifest: &WorkspaceManifest) -> Result<()>` — creates parent dirs
- [x] `fn merge(existing: Option<WorkspaceManifest>, discovered: Vec<DiscoveredExtension>) -> WorkspaceManifest`
  - New extensions: add with `enabled` = `true` if system, `false` if user/workspace
  - Existing (matched by id): keep `enabled` state, update checksum/path/name/slot
  - In manifest but not discovered: remove

### Step 7: CLI — `src/cli.rs` + `src/main.rs`

- [x] Define clap structs:

  ```rust
  #[derive(Parser)]
  struct Cli {
      #[arg(short, long)]
      workspace: Option<PathBuf>,
      #[command(subcommand)]
      command: Command,
  }

  #[derive(Subcommand)]
  enum Command {
      Extensions {
          #[command(subcommand)]
          action: ExtensionAction,
      },
  }

  #[derive(Subcommand)]
  enum ExtensionAction {
      List,
      Enable { id: String },
      Disable { id: String },
      Inspect { id: String },
  }
  ```

- [x] Implement shared preamble: `fn scan_and_load_manifest(ur_root, workspace, engine) -> Result<WorkspaceManifest>` — discovers, merges, saves, returns manifest
- [x] Implement `list`: print table (id, name, slot, source, enabled)
- [x] Implement `enable`:
  - Find extension by id in manifest, error if not found or already enabled
  - For exactly-1 slots: if another extension in the same slot is already enabled, disable it (switch semantics)
  - Set `enabled = true`, save
- [x] Implement `disable`:
  - Find extension by id in manifest, error if not found or already disabled
  - Count enabled extensions in the same required slot
  - If this is the last one → error: `"cannot disable {id}: it is the only {slot} provider"`
  - Set `enabled = false`, save
- [x] Implement `inspect`: print all fields for a single extension
- [x] Wire `main.rs` to parse CLI args and dispatch

**Files:** `src/cli.rs`, `src/main.rs`

### Step 8: Smoke Test

```bash
# Build all extensions
for dir in extensions/system/session-jsonl \
           extensions/system/compaction-llm \
           extensions/system/llm-openai \
           extensions/user/llm-anthropic \
           workspace/.ur/extensions/test-extension; do
  cargo build --manifest-path $dir/Cargo.toml --target wasm32-wasip2 --release
done

# Build host
cargo build

# List — system enabled, user+workspace disabled
UR_ROOT=. target/debug/ur -w workspace extensions list

# Enable anthropic (second llm-provider — allowed)
UR_ROOT=. target/debug/ur -w workspace extensions enable llm-anthropic

# Disable openai (anthropic still covers llm-provider — allowed)
UR_ROOT=. target/debug/ur -w workspace extensions disable llm-openai

# Disable anthropic — ERROR: last llm-provider
UR_ROOT=. target/debug/ur -w workspace extensions disable llm-anthropic

# Disable compaction-llm — ERROR: only compaction-provider
UR_ROOT=. target/debug/ur -w workspace extensions disable compaction-llm

# Disable session-jsonl — ERROR: only session-provider
UR_ROOT=. target/debug/ur -w workspace extensions disable session-jsonl

# Inspect manifest
cat workspaces/$(pwd | sed 's|/|_|g' | sed 's/^_//')/manifest.json | python3 -m json.tool

# Enable test-extension (no slot — always allowed)
UR_ROOT=. target/debug/ur -w workspace extensions enable test-extension

# Inspect a specific extension
UR_ROOT=. target/debug/ur -w workspace extensions inspect session-jsonl
```

## Acceptance Criteria

- [ ] `extensions list` shows all 5 extensions with correct source tier and enabled/disabled state
- [ ] System extensions (session-jsonl, compaction-llm, llm-openai) default to enabled
- [ ] User (llm-anthropic) and workspace (test-extension) extensions default to disabled
- [ ] `extensions enable llm-anthropic` succeeds
- [ ] `extensions disable llm-openai` succeeds when llm-anthropic is also enabled
- [ ] `extensions disable llm-anthropic` fails when it's the last llm-provider
- [ ] `extensions disable compaction-llm` fails (only compaction-provider)
- [ ] `extensions disable session-jsonl` fails (only session-provider)
- [ ] Manifest at `$UR_ROOT/workspaces/<escaped>/manifest.json` is correct JSON
- [ ] Adding a new `.wasm` and re-running `list` discovers it automatically
- [ ] No explicit init command needed — first CLI command creates manifest and directories
- [ ] `extensions inspect <id>` shows id, name, slot, source, path, checksum, enabled state
- [ ] Duplicate extension IDs produce a hard error
- [ ] Unknown slot names produce a hard error

## Dependencies & Risks

- **wasmtime loading overhead:** Every CLI command loads all `.wasm` files to call `register()`. Fine for 5 extensions. May need checksum-based caching later.
- **WIT breaking change:** Adding `slot` to manifest changes the guest interface. All extensions must be rebuilt after the WIT change.
- **No tool/command collision detection:** Brainstorm requires hard errors on duplicate tool names. Deferred — these stubs register no tools.
- **Extension build workflow:** Each extension requires a separate `cargo build --target wasm32-wasip2 --release`. A build script or Makefile may be warranted but is out of scope.

## References

- [Extension Conflicts Brainstorm](../brainstorms/2026-03-21-extension-conflicts-and-management-brainstorm.md) — key decisions
- [Basic Extension Loading Plan](2026-03-21-feat-basic-extension-loading-plan.md) — Phase 1 foundation
- [UR.md](../UR.md) — full specification
- [src/extension_host.rs](../../src/extension_host.rs) — existing WASM loading code
- [wit/world.wit](../../wit/world.wit) — current WIT interface
