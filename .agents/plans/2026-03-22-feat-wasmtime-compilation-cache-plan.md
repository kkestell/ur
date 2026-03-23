---
title: "feat: Enable wasmtime compilation cache"
type: feat
date: 2026-03-22
---

# feat: Enable wasmtime compilation cache

## Overview

Enable wasmtime's built-in content-addressed compilation cache so that
`.wasm` → native compilation is only performed once per unique component.
Subsequent loads hit the cache, eliminating the expensive recompilation
that currently happens on every `ur` invocation.

## Problem Statement

Every time `ur` runs, `Component::from_file(engine, path)` at
`src/extension_host.rs:189` recompiles each WASM extension from scratch.
With 5 extensions, this causes significant CPU load and latency — most
noticeably during repeated smoke test runs where extensions rarely change.

## Proposed Solution

Replace `Engine::default()` with a configured `Engine` that has
`cache_config_load_default()` enabled. Wasmtime will then automatically
cache compiled native artifacts in `~/.cache/wasmtime/`, keyed by engine
configuration + WASM content hash.

### Changes

**`Cargo.toml`** — Enable the `cache` feature on `wasmtime`:

```toml
wasmtime = { version = "43.0.0", features = ["cache"] }
```

**`src/main.rs:34`** — Replace `Engine::default()` with:

```rust
let mut config = wasmtime::Config::new();
config.cache_config_load_default()?;
let engine = Engine::new(&config)?;
```

That's it. Two files, ~5 lines changed.

## Acceptance Criteria

- [x] `wasmtime` dependency includes the `cache` feature — `Cargo.toml`
- [x] Engine is constructed with compilation cache — `src/main.rs`
- [ ] First `ur` invocation populates cache (runs at normal speed)
- [ ] Second invocation with unchanged extensions loads significantly faster
- [x] `make verify` passes (build, test, clippy, fmt)
- [ ] Smoke test passes

## Context

- **Brainstorm:** `.agents/brainstorms/2026-03-22-wasmtime-compilation-cache-brainstorm.md`
- **Engine construction:** `src/main.rs:34`
- **Component loading:** `src/extension_host.rs:188-189`
- **Cache location:** `~/.cache/wasmtime/` (wasmtime's default, platform-appropriate)
