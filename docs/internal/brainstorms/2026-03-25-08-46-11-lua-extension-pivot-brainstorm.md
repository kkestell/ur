# Lua Extension Pivot

**Date:** 2026-03-25
**Status:** Brainstorm complete, ready for planning

## How Might We

How might we simplify the extension system by replacing the WASM Component Model
with embedded Lua, while preserving the capabilities extensions need?

## Why This Approach

The WASM extension system (wasmtime + WIT + Component Model + wit-bindgen) is
causing friction across every dimension: build times, authoring ceremony,
debugging, and onboarding cost. Extensions in Rust targeting wasm32-wasip2
require authors to understand the Component Model, WIT interfaces, and WASM
toolchain — a steep barrier for what should be a lightweight plugin system.

Simultaneously, the current extension model is over-scoped. LLM providers,
session storage, and compaction don't benefit from being extensions — they're
core infrastructure that should live in the host. Extensions should focus on
what users actually want to customize: **tools** and **lifecycle hooks**.

Lua (specifically Luau via mlua) is a proven extension language used by Neovim,
Redis, Roblox, and others. It offers built-in sandboxing, type annotations, and
a fast, embeddable runtime with excellent Rust integration.

## Validated Assumptions

1. Remove ALL WASM infrastructure (wasmtime, WIT, wit-bindgen, wasm32-wasip2 targets, extension Cargo.tomls)
2. LLM providers (Google, OpenRouter), session storage (JSONL), and compaction move to native Rust in the host
3. Luau via mlua with built-in sandboxing (`Lua::sandbox()`)
4. Extensions provide: **tools** + **9 lifecycle hooks** (from prior brainstorm)
5. Capability declarations preserved: network, fs-read, fs-write
6. Directory-based packaging with `extension.toml` manifest
7. Full host API exposed to Lua: log, complete, session-read, fs (gated), http (gated)
8. 3-tier discovery preserved: system / user / workspace
9. One test Lua extension shipped for validation (tools + all hooks)
10. Slot system and cardinality rules dropped
11. Clean break — no WASM coexistence or migration period
12. One sandboxed Lua VM (mlua::Lua instance) per extension

## Constraints

- **Clean break**: No migration period. WASM is removed entirely and Lua replaces it.
- **Greenfield posture**: No backwards compatibility concerns. Refactor as needed.
- **Scope boundary**: Future system tools (bash, read_file, write_file, edit_file) are out of scope for this pivot. They'll eventually be Lua extensions but not now.

## Key Decisions

### 1. Core vs. Extension Boundary

**Decision:** LLM providers, session storage, and compaction become native Rust
modules in the host. Extensions focus exclusively on tools and lifecycle hooks.

**Why:** These providers are core infrastructure — they need deep integration
with streaming, async I/O, and the session model. Making them extensions added
complexity without real extensibility benefit. Pulling them into the host
eliminates the slot system, cardinality rules, and provider routing complexity.

### 2. Luau via mlua

**Decision:** Use Luau (Roblox's Lua fork) via the mlua crate.

**Why:** Luau provides built-in sandboxing via `Lua::sandbox()`, type
annotations for better extension authoring, and good performance. mlua has
excellent Rust integration including async support (Lua coroutines yield while
Rust futures resolve). Other options considered:
- LuaJIT: fastest but stuck at Lua 5.1, no built-in sandbox
- Lua 5.4: standard but sandboxing requires manual env restriction

### 3. Extension Authoring Model: Manifest + Registration API

**Decision:** Extensions have a declarative `extension.toml` for static metadata
and an `init.lua` that imperatively registers tools and hooks via a host API.

**Why:** Clean separation between what the host needs to know at discovery time
(id, name, capabilities) and runtime behavior (handlers). The imperative
registration API allows conditional registration based on config or environment.
This pattern is familiar to plugin authors across ecosystems.

Example extension structure:

```
weather/
  extension.toml
  init.lua
```

```toml
# extension.toml
[extension]
id = "weather"
name = "Weather Tools"
capabilities = ["network"]
```

```lua
-- init.lua
local ur = require("ur")

ur.tool("get_weather", {
  description = "Get current weather for a location",
  parameters = {
    location = { type = "string", required = true },
  },
  handler = function(args)
    local resp = ur.http.get("https://api.weather.example/" .. args.location)
    return resp.body
  end,
})

ur.hook("before_completion", function(ctx)
  ur.log("completing with model: " .. ctx.model)
  return { action = "pass" }
end)

ur.hook("after_tool", function(ctx)
  ur.log("tool " .. ctx.tool_name .. " returned: " .. tostring(ctx.result))
  return { action = "pass" }
end)
```

### 4. One VM Per Extension

**Decision:** Each extension gets its own sandboxed `mlua::Lua` instance.

**Why:** Complete state isolation between extensions. No cross-extension
interference. Simpler security model — each VM's sandbox is configured based on
that extension's declared capabilities. Slightly more memory than a shared VM
with separate environments, but much simpler and safer.

### 5. Host API (`ur` Module)

**Decision:** Expose a full host API to Lua, gated by capabilities:

| API | Requires |
|-----|----------|
| `ur.log(msg)` | (always available) |
| `ur.tool(name, spec)` | (always available) |
| `ur.hook(name, fn)` | (always available) |
| `ur.config` | (always available) |
| `ur.complete(messages, opts)` | (always available) |
| `ur.session.load(id)` | (always available, read-only) |
| `ur.session.list()` | (always available, read-only) |
| `ur.http.get(url, opts)` | network |
| `ur.http.post(url, body, opts)` | network |
| `ur.fs.read(path)` | fs-read |
| `ur.fs.write(path, content)` | fs-write |
| `ur.fs.list(path)` | fs-read |

If an extension calls a gated API without the required capability, it errors.

### 6. Lifecycle Hooks (from prior brainstorm, adapted)

The 9 hook points from the prior ReAct loop brainstorm carry forward:

| Hook | Input | Can Mutate | Can Reject |
|------|-------|-----------|------------|
| before_completion | messages, model, settings, tools | messages, model, settings, tools | yes |
| after_completion | messages, model, response | response | no |
| before_tool | tool name, args, call ID | args | yes |
| after_tool | tool name, args, call ID, result | result | no |
| before_session_load | session ID | — | yes |
| after_session_load | session ID, messages | messages | no |
| before_session_append | session ID, message | message | yes |
| before_compaction | messages | messages | yes |
| after_compaction | original messages, compacted | compacted | no |

Hook return values use the same three-way pattern, expressed as Lua tables:
- `{ action = "pass" }` — no changes
- `{ action = "modify", ... }` — modified data in table fields
- `{ action = "reject", reason = "..." }` — abort (before-hooks only)

### 7. Discovery

**Decision:** Preserve 3-tier discovery (system/user/workspace). Scan for
directories containing `extension.toml`.

Paths:
- System: `~/.ur/extensions/system/`
- User: `~/.ur/extensions/user/`
- Workspace: `.ur/extensions/workspace/`

Discovery reads only `extension.toml` — no Lua execution during discovery.

### 8. Hook Ordering

**Decision:** Carry forward the user-defined, per-hook-point ordering from the
prior brainstorm. Ordering persisted in the workspace manifest.

## What Gets Removed

- `wit/world.wit` and the entire `wit/` directory
- All extension `Cargo.toml` files and Rust source under `extensions/`
- `src/extension_host.rs` (wasmtime loading, capability enforcement)
- `src/slot.rs` (slot system, cardinality)
- wasmtime, wasmtime-wasi, wasmtime-wasi-http dependencies
- wit-bindgen dependency (in extension crates)
- All Makefile wasm targets (`build-extensions`, wasm32-wasip2 compilation)
- Extension install logic that copies `.wasm` files

## What Gets Added

- `mlua` dependency (with `luau` and `async` features)
- New Lua host runtime module (replaces `extension_host.rs`)
- New discovery module (replaces wasm discovery in `discovery.rs`)
- Extension manifest parser (`extension.toml`)
- Host API module exposing `ur.*` to Lua
- Native Rust LLM provider modules (Google, OpenRouter)
- Native Rust session storage module (JSONL)
- Native Rust compaction module
- One test Lua extension (tools + all hooks)

## What Gets Refactored

- `src/discovery.rs` — scan for `extension.toml` instead of `.wasm`
- `src/manifest.rs` — adapt to new extension model (no slots)
- `src/extension_settings.rs` — adapt to Lua config model
- Makefile — remove all wasm targets, simplify build/install
- `Cargo.toml` — remove wasmtime deps, add mlua

## Failure Modes

1. **Lua performance for hot paths**: Lifecycle hooks run on every turn. If hook
   execution adds measurable latency, it degrades the experience. Mitigation:
   hooks are simple Lua functions, not complex programs. Profile early.

2. **Async complexity**: Host API functions (complete, http) are async. mlua's
   async support uses Lua coroutines under the hood. If this is flaky or has
   edge cases, it could be painful. Mitigation: mlua's async is well-tested;
   start with sync-only and add async incrementally if needed.

3. **Sandboxing gaps**: Luau's sandbox may not cover all vectors (e.g., CPU
   exhaustion, memory bombs). Mitigation: mlua supports instruction count hooks
   and memory limits. Configure these per-VM.

4. **Extension authoring without types**: Luau has type annotations but they're
   optional. Extension authors may struggle without the type safety WIT provided.
   Mitigation: provide a well-documented `ur` API with Luau type stubs.

## Open Questions

1. **Extension config delivery**: How does user config (from `~/.ur/config.toml`)
   reach extensions? Via `ur.config` table populated before `init.lua` runs?

2. **Error handling convention**: When a tool handler errors, what does the LLM
   see? A ToolResult with error text (like the prior brainstorm suggested)?

3. **Hot reload**: Should `ur` watch extension files and reload on change during
   development? Not required initially but worth designing for.

4. **Luau type stubs**: Should we ship a `ur.d.luau` type definition file so
   extension authors get autocomplete?

## Related Documents

- [ReAct Loop Extension Hooks Brainstorm](docs/internal/brainstorms/2026-03-23-16-15-50-react-loop-hooks-brainstorm.md)
