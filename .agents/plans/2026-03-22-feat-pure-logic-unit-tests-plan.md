---
title: "feat: Add pure-logic unit tests"
type: feat
date: 2026-03-22
---

# Add Pure-Logic Unit Tests

## Overview

The codebase has zero tests. This plan adds `#[cfg(test)]` unit test modules to the four modules with testable pure logic: `slot`, `manifest` (merge/enable/disable), `config` (parsing and validation), and `model` (role resolution). No mocking or external fixtures required.

## Acceptance Criteria

- [x] `cargo test` passes with all new tests
- [x] Each module below has a `#[cfg(test)] mod tests` block
- [x] Tests cover happy paths and error/edge cases as listed

## Modules

### 1. `src/slot.rs`

Add `#[cfg(test)] mod tests` at the bottom of the file.

**Tests:**

- [x] `find_slot` — returns `Some` for each known slot (`session-provider`, `compaction-provider`, `llm-provider`)
- [x] `find_slot` — returns `None` for unknown slot name
- [x] `validate_slot_name` — `Ok` for known slot, `Err` for unknown slot
- [x] `validate_required_slots` — passes when all required slots have enough enabled providers
- [x] `validate_required_slots` — fails when a required `ExactlyOne` slot has 0 providers
- [x] `validate_required_slots` — fails when a required `ExactlyOne` slot has 2 providers
- [x] `validate_required_slots` — fails when a required `AtLeastOne` slot has 0 providers
- [x] `validate_required_slots` — passes when `AtLeastOne` slot has 2+ providers
- [x] `validate_required_slots` — disabled entries are not counted

### 2. `src/manifest.rs`

Add `#[cfg(test)] mod tests` at the bottom of the file. These tests cover merge logic and state transitions only (no filesystem).

**Helper:** Build a `ManifestEntry` factory function to reduce boilerplate:

```rust
fn entry(id: &str, slot: Option<&str>, source: &str, enabled: bool) -> ManifestEntry {
    ManifestEntry {
        id: id.to_owned(),
        name: id.to_owned(),
        slot: slot.map(str::to_owned),
        source: source.to_owned(),
        wasm_path: String::new(),
        checksum: String::new(),
        enabled,
    }
}
```

Similarly, a `DiscoveredExtension` factory:

```rust
fn discovered(id: &str, slot: Option<&str>, source: SourceTier) -> DiscoveredExtension {
    DiscoveredExtension {
        id: id.to_owned(),
        name: id.to_owned(),
        slot: slot.map(str::to_owned),
        source,
        wasm_path: PathBuf::new(),
        checksum: String::new(),
    }
}
```

**`merge` tests:**

- [x] Fresh merge (no existing manifest) — system extensions default enabled, user/workspace default disabled
- [x] Re-merge preserves existing enabled state for known extensions
- [x] Extensions no longer discovered are dropped from the merged result
- [x] New extensions added alongside existing ones get correct defaults

**`enable` tests:**

- [x] Enabling a disabled extension succeeds
- [x] Enabling an already-enabled extension returns error
- [x] Enabling in an `ExactlyOne` slot disables the current occupant (switch semantics)
- [x] Enabling in an `AtLeastOne` slot does not disable others
- [x] Enabling an extension not in the manifest returns error

**`disable` tests:**

- [x] Disabling an enabled extension succeeds
- [x] Disabling an already-disabled extension returns error
- [x] Disabling the last provider of a required slot returns error
- [x] Disabling one of multiple providers in a required `AtLeastOne` slot succeeds
- [x] Disabling an extension not in the manifest returns error

**`find_entry` / `find_entry_index` tests:**

- [x] Returns the correct entry for a known id
- [x] Returns error for an unknown id

**`escape_workspace_path` test:**

- [x] Slashes replaced with underscores, leading underscore stripped

### 3. `src/config.rs`

Add `#[cfg(test)] mod tests` at the bottom of the file.

**`parse_model_ref` tests:**

- [x] Valid ref `"anthropic/claude-sonnet-4-6"` → `Some(("anthropic", "claude-sonnet-4-6"))`
- [x] Empty provider `"/model"` → `None`
- [x] Empty model `"provider/"` → `None`
- [x] No slash `"justprovider"` → `None`
- [x] Multiple slashes `"a/b/c"` → `None`

**`resolve_role` tests (on `UserConfig`):**

- [x] Returns configured role when present
- [x] Returns `None` for unconfigured role

**`validate_integer` tests:**

- [x] Value within bounds → `Ok`
- [x] Value below min → `Err`
- [x] Value above max → `Err`
- [x] Value at exact min/max boundaries → `Ok`

**`validate_enum` tests:**

- [x] Allowed value → `Ok`
- [x] Disallowed value → `Err`

**`UserConfig::settings_for` tests:**

Requires constructing `ModelDescriptor` values with setting schemas. Since `wit_types` are generated types, verify these are constructible in test context. If not, skip and note as a follow-up.

- [x] Returns defaults when no overrides configured
- [x] Returns overridden values when config has provider settings
- [x] Rejects invalid override values (wrong type, out of range)

### 4. `src/model.rs`

Add `#[cfg(test)] mod tests` at the bottom of the file.

**`resolve_role` tests:**

- [x] Explicit role mapping returns that mapping
- [x] Unknown role falls back to `"default"` if configured
- [x] No config falls back to first provider's default model
- [x] No providers at all → error

**`find_descriptor` tests:**

- [x] Returns descriptor for known provider/model pair
- [x] Returns `None` for unknown provider
- [x] Returns `None` for unknown model within known provider

**`parse_setting_value` tests:**

- [x] Integer string parses correctly within bounds
- [x] Integer string out of bounds → `Err`
- [x] Non-integer string for integer schema → `Err`
- [x] Valid enum string → `Ok`
- [x] Invalid enum string → `Err`
- [x] Boolean `"true"` / `"false"` parse correctly
- [x] Non-boolean string for boolean schema → `Err`

## Notes

- All tests use `#[cfg(test)] mod tests` inline in each source file (idiomatic Rust)
- Use `assert!`, `assert_eq!`, and pattern matching on `Result` for assertions
- `model.rs` tests for `resolve_role` and `find_descriptor` require constructing `wit_types::ModelDescriptor` — verify the generated type is constructible without WASM. If the generated struct has private fields, these specific tests may need to be deferred
- No new dependencies needed — `#[cfg(test)]` uses the standard library test harness

## References

- [src/slot.rs](src/slot.rs) — slot definitions, find, validate
- [src/manifest.rs](src/manifest.rs) — merge, enable, disable, find
- [src/config.rs](src/config.rs) — parse_model_ref, validate_integer, validate_enum, settings_for
- [src/model.rs](src/model.rs) — resolve_role, find_descriptor, parse_setting_value
