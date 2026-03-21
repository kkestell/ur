---
title: "Basic Extension Loading"
type: feat
date: 2026-03-21
---

# Basic Extension Loading

Prove the plugin round-trip works end-to-end: load a `.wasm` component, call an export, and have the guest call back into a host import. No REPL, no CLI parsing, no event bus — just the bare mechanical proof.

## WIT Definition

A minimal subset of the full `ur:plugin@0.1.0` contract. Enough to demonstrate bidirectional calls.

```wit
// wit/world.wit
package ur:plugin@0.1.0;

interface host {
    /// Test-only host import — proves guest→host callback works.
    /// Not part of the real spec; remove once real host imports land.
    log: func(msg: string);
}

record plugin-manifest {
    id: string,
    name: string,
}

interface plugin {
    register: func() -> plugin-manifest;
    call-tool: func(name: string, args-json: string) -> result<string, string>;
}

world ur-plugin {
    import host;
    export plugin;
}
```

Key choices:
- `plugin-manifest` is deliberately minimal — just `id` and `name`. The full manifest (tools, commands, events, capabilities) comes later.
- `call-tool` matches the real spec's signature (`result<string, string>`).
- `log` is a throwaway host import to prove the callback path. It gets replaced by real host imports (`read-file`, `write-file`, etc.) in later phases.

## Layout

```
wit/
  world.wit                 # shared WIT (used by both host and guest)
src/
  main.rs                   # load plugin, call register + call-tool, print results
  plugin_host.rs            # wasmtime setup: engine, linker, store, bindings
plugins/
  test-plugin/
    Cargo.toml              # guest crate (cdylib, wit-bindgen)
    src/
      lib.rs                # implements register + call-tool, calls log()
```

`plugins/test-plugin/` is **not** a workspace member — it targets `wasm32-wasip2` and is built separately.

## Dependencies

Host (`ur`):
- `wasmtime` (with default features — includes `component-model`)
- `wasmtime-wasi` (needed even for pure-compute guests; Rust stdlib links WASI imports)

Guest (`test-plugin`):
- `wit-bindgen`

## Host (`src/main.rs` + `src/plugin_host.rs`)

`plugin_host.rs` encapsulates wasmtime setup:
- Create `Engine` (default config)
- Compile `Component` from a `.wasm` file path
- Set up `Linker` with WASI + host imports
- Create `Store` with host state
- Instantiate component, return typed bindings

Host state struct holds `WasiCtx` + `ResourceTable` (required by wasmtime-wasi) and implements the generated `Host` trait for the `log` import.

`main.rs`:
1. Build the test plugin: `cargo build -p test-plugin --target wasm32-wasip2 --release` (or expect it pre-built)
2. Load `plugins/test-plugin/target/wasm32-wasip2/release/test_plugin.wasm`
3. Call `register()` → print the manifest
4. Call `call-tool("hello", "{}")` → plugin calls `log()` on the host (visible in stdout), returns a result → print it

For the MVP, the `.wasm` path is hardcoded or passed as a CLI argument. No plugin discovery.

## Guest (`plugins/test-plugin/src/lib.rs`)

```rust
// Generates bindings from ../../wit/
// Implements:
//   register() → PluginManifest { id: "test", name: "Test Plugin" }
//   call-tool("hello", _) → calls host log(), returns Ok("hello from test plugin")
//   call-tool(_, _) → Err("unknown tool: {name}")
```

`Cargo.toml` must specify `crate-type = ["cdylib"]` and depend on `wit-bindgen`. The `wit` path is `../../wit` (relative to the guest crate root).

## Build & Run

```bash
# 1. Ensure wasm target is installed
rustup target add wasm32-wasip2

# 2. Build the guest
cargo build --manifest-path plugins/test-plugin/Cargo.toml --target wasm32-wasip2 --release

# 3. Build and run the host
cargo run
```

## Acceptance Criteria

- [x] `cargo build` succeeds for the host (no warnings)
- [x] Guest builds with `cargo build --target wasm32-wasip2 --release`
- [x] Running `cargo run` loads the test plugin `.wasm` and calls `register()`
- [x] `register()` returns `PluginManifest { id: "test", name: "Test Plugin" }` — printed to stdout
- [x] `call-tool("hello", "{}")` triggers the guest to call `log("...")` on the host — `[host log] ...` appears in stdout
- [x] `call-tool("hello", "{}")` returns `Ok("hello from test plugin")` — printed to stdout
- [x] `call-tool("unknown", "{}")` returns `Err("unknown tool: unknown")` — printed to stdout

## What This Doesn't Include

- Core types from Phase 1 (not needed yet)
- CLI argument parsing (hardcoded path is fine)
- REPL
- Event bus
- Plugin discovery
- Plugin capabilities / approval
- Real host imports (`read-file`, `write-file`, etc.)
- `reload` built-in

All of the above are subsequent phases. This plan is exclusively about proving the wasmtime component model pipeline works.

## Context

- [UR.md Plugin SDK](../UR.md) lines 104–227 — full WIT contract this is a subset of
- [HIGH_LEVEL_PLAN.md Phase 2](../HIGH_LEVEL_PLAN.md) lines 13–24 — scope this aligns with
- [Plugin Permissions Brainstorm](../brainstorms/2026-03-21-plugin-permissions-and-tool-approval-brainstorm.md) — capability model (deferred)

## References

- [wasmtime component::bindgen! docs](https://docs.rs/wasmtime/latest/wasmtime/component/macro.bindgen.html)
- [wit-bindgen generate! docs](https://docs.rs/wit-bindgen/latest/wit_bindgen/macro.generate.html)
- [WIT syntax reference](https://component-model.bytecodealliance.org/design/wit.html)
- wasmtime 43.0.0, wit-bindgen 0.54.0 (current as of 2026-03-20)
