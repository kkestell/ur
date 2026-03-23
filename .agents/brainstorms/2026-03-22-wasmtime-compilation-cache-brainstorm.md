# Wasmtime Compilation Cache

**Date:** 2026-03-22
**Status:** Ready for planning

## What We're Building

Enable wasmtime's built-in compilation cache so that WASM extensions only
undergo the expensive `.wasm` -> native compilation once. Subsequent loads
of unchanged extensions hit the cache and skip compilation entirely.

## Why This Approach

The smoke test (and any ur invocation) currently calls
`Component::from_file(engine, path)` for every extension on every run.
Wasmtime recompiles the WASM component to native code each time -- this is
the primary source of CPU load and latency during extension loading.

Wasmtime provides a built-in content-addressed cache
(`Config::cache_config_load_default()`) that stores compiled artifacts in
`~/.cache/wasmtime/`. It keys on engine configuration + WASM content hash,
handling invalidation automatically when the WASM changes or the engine
config changes (e.g., wasmtime version upgrade).

We chose this over manual `serialize`/`deserialize` because:
- ~3 lines of code vs. a new caching subsystem
- No `unsafe` blocks required
- Wasmtime handles cache eviction and invalidation
- YAGNI -- we can always migrate to manual caching later if we need
  control over cache location

## Key Decisions

- **Cache in ur, not the harness** -- this benefits all ur invocations, not
  just smoke tests.
- **Use wasmtime's built-in cache** -- `Config::cache_config_load_default()`
  rather than manual serialize/deserialize.
- **Single change point** -- wherever the `Engine` is constructed, enable
  the cache config on it.

## Open Questions

- Does the `cache` feature need to be explicitly enabled in the wasmtime
  dependency? (Check `Cargo.toml` for current feature flags.)
- Should we log a message when the cache is populated vs. hit, for
  observability during development?
