# Extension Conflicts and Management

**Date:** 2026-03-21
**Status:** Brainstorm complete

## What We're Building

A conflict-free extension loading system with host-declared slots, manifest-based management, and three-tier discovery. The goal is to make "everything is an extension" work without the pitfalls that plagued Eclipse, WordPress, and VS Code.

## Key Decisions

### 1. Host-declared slots with cardinality

The host defines a fixed set of typed slots that extensions can fill:

| Slot | Cardinality | Required | Example |
|------|------------|----------|---------|
| `session-provider` | Exactly 1 | Yes | `session-jsonl` |
| `compaction-provider` | Exactly 1 | TBD | `compaction-llm` |
| `llm-provider` | At least 1 | Yes | `llm-gemini`, `llm-openai` |

- If a required slot is unfilled at startup, the host refuses to start.
- Cardinality is enforced at the CLI level (enable/disable time), not just at load time.
- Enabling a competing singleton prompts: "Switch session-provider from A to B?"
- Cannot disable the last provider in a required slot.

### 2. Slots gate their associated events

- Filling a slot grants exclusive subscription rights to that slot's gate events.
- Example: only the `session-provider` can handle `session:resolve`.
- Notification and filter events remain open to all extensions.
- This prevents behavioral conflicts, not just naming conflicts.

### 3. Tool and command names are globally unique

- Two extensions registering a tool with the same name is a hard error at load time.
- Same for slash commands.
- No namespacing, no last-one-wins — just fail fast.

### 4. Three-tier extension discovery

| Source | Default state |
|--------|--------------|
| `$UR_ROOT/extensions/system` | Enabled |
| `$UR_ROOT/extensions/user` | Available, disabled |
| `$WORKSPACE/.ur/extensions` | Available, disabled |

`$UR_ROOT` defaults to `~/.ur`, overridable via `UR_ROOT` env var.

### 5. Manifest-based state management

- Full-snapshot manifests stored in `$UR_ROOT/workspaces/<escaped-workspace-path>/manifest.json`.
- Contains: all discovered extensions, their source tier, checksums, and enabled/disabled state.
- Manifests live outside the workspace so extensions cannot modify their own state.
- Discovery is automatic — drop a `.wasm` in the right directory and it appears in `ur extension list`.

### 6. CLI management

```
ur extension list       # all discovered extensions, source, enabled/disabled, slot
ur extension enable     # enable (with cardinality enforcement)
ur extension disable    # disable (with cardinality enforcement)
ur extension inspect    # tools, events, slot, checksum, capabilities
```

### 7. Deferred (YAGNI)

- **Generic extension state persistence** — extensions that need state (like session) already use `read-file`/`write-file` host imports. No KV store or per-extension storage yet.
- **Inter-extension communication** — no message bus, no typed cross-extension interfaces. Defer until a concrete use case demands it.
- **Extension-declarable slots** — slots are fixed in the host. If real-world usage shows extensions need custom slots, add later.

## Why This Approach

The slot system is inspired by Eclipse's extension points but without the complexity of a schema language. The three-tier discovery with manifest-based management avoids WordPress's "last plugin loaded wins" chaos. Hard errors on collision (instead of warnings) force conflicts to be resolved at configuration time, not at runtime when they manifest as subtle bugs.

Storing manifests outside the workspace prevents extensions from modifying their own enabled state, which is important given that extensions run in a WASM sandbox but could theoretically write to workspace files.

## Open Questions

- **LLM provider model** — multiple providers with selection logic is a distinct problem from singleton slots. Needs its own brainstorm.
- **Manifest file format** — JSON? TOML? Needs to be human-readable for debugging.
- **Checksum approval workflow** — UR.md describes prompting on first load or hash change. How does this interact with the manifest?
- **`-p` / `--plugin` CLI flag** — UR.md allows loading plugins via CLI flag. How does this interact with the manifest? Probably session-scoped override that bypasses the manifest.
- **Slot list completeness** — what other slots beyond session-provider, compaction-provider, and llm-provider?

## Prior Art Considered

| System | Lesson taken | Lesson avoided |
|--------|-------------|----------------|
| VS Code | Lazy activation, per-extension storage | No priority system, silent conflicts |
| Eclipse | Extension points with cardinality | Over-engineered schema, slow startup |
| WordPress | Numeric priority hooks | Silent "last loaded wins" conflicts |
| Terraform | Non-overlapping provider namespaces | Only works when domains don't overlap |
| Express.js | Linear middleware pipeline | Order-dependent, hard to reason about |
| Emacs | Maximum extensibility | Global mutable state, no isolation |
