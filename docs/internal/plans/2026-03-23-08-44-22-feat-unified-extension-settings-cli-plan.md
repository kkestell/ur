---
title: "feat: Unified extension settings CLI"
type: feat
date: 2026-03-23
---

# feat: Unified Extension Settings CLI

## Overview

Replace the fragmented `ur config`, `ur model config`, and `ur model setting` commands with a single uniform `ur extension <id> config {list|get|set}` interface. Move role management to `ur role`. Eliminate `ur model` entirely. Extensions become fully self-describing: API keys are secret settings, model metadata is read-only settings, and model-specific configuration uses dotted-key namespacing.

## Problem Statement / Motivation

The current CLI has three separate nouns for what is conceptually one operation — configuring extensions:

- `ur config set-key <provider>` — API keys
- `ur model config <role>` / `ur model setting <role> <key> <value>` — model settings
- `ur model info <model_ref> <property>` — model metadata queries

This creates naming confusion and will not scale to non-LLM extensions that need configuration. Since extensions are the fundamental abstraction, configuration should be organized by extension, not by an ad-hoc taxonomy of "config", "model", and "info".

## Proposed Solution

### New CLI shape

```
ur role list                                        # show role -> model mappings
ur role set <role> <provider/model>                 # assign a role
ur role get <role>                                  # query a role

ur extension list                                   # all discovered extensions
ur extension <id> enable                            # enable an extension
ur extension <id> disable                           # disable an extension
ur extension <id> inspect                           # show extension details
ur extension <id> config list [pattern]             # list settings (optional glob)
ur extension <id> config get <key>                  # read a setting value
ur extension <id> config set <key> [value]          # write (prompts if secret)

ur run                                              # execute a single agent turn
```

### Removed commands

- `ur model` (all subcommands) — replaced by `ur role` and `ur extension <id> config`
- `ur config` — API keys are now `ur extension <id> config set api_key`
- `ur extensions` (plural) — renamed to `ur extension` (singular noun)

### New WIT surface

```wit
record setting-descriptor {
    key: string,
    name: string,
    description: string,
    schema: setting-schema,
    secret: bool,       // store in keyring, prompt without echo
    readonly: bool,     // reject set, display in listings
}

// New export on the base extension interface:
list-settings: func() -> list<setting-descriptor>;
```

### Config.toml restructure

```toml
# Before
[providers.google.gemini-flash]
thinking_level = "high"

# After
[extensions.google]
"gemini-flash.thinking_level" = "high"
```

The `[roles]` section is unchanged.

## Technical Considerations

### Architecture

- **`list-settings()` on the base extension interface** means all extension types (LLM, session, compaction, general) can declare settings. Non-LLM extensions return `[]` if they have nothing to configure.
- **LLM extensions flatten their namespace**: `api_key` (secret), `<model>.thinking_level` (enum), `<model>.context_window_in` (readonly integer), etc. The host passes keys through opaquely — it does not parse the dotted structure.
- **`model-descriptor` shrinks** to identity fields only: `id`, `name`, `description`, `is_default`. Settings and metadata move to `list-settings()`. This eliminates the duplication between model-level settings and extension-level settings.
- **`list-models()` still exists** — needed for role resolution (enumerating available models). It just stops carrying settings and metadata.
- **`list-settings()` is called after `init()`** — LLM extensions with dynamic catalogs (OpenRouter) need API keys before they can enumerate model-specific settings.

### Config storage

- Mutable settings: stored in `config.toml` under `[extensions.<id>]` with dotted keys as TOML keys.
- Secret settings: stored in OS keyring (existing `keyring.rs` infra). The key format remains `set_api_key(extension_id, value)`.
- Read-only settings: never stored. Always queried live from the extension via `list-settings()`. Displayed in `config list` output but `config set` rejects them.

### Glob filtering

`ur extension <id> config list [pattern]` accepts an optional glob pattern matched against the dotted key namespace. Examples: `gemini-flash.*`, `*.thinking_level`, `openai/*`. The `glob` crate handles matching. Model names may contain slashes (OpenRouter: `openai/gpt-5`), and the dot separates model name from setting name.

### Engineering Quality

| Principle | Application |
|-----------|-------------|
| **SRP** | `cli.rs` defines shapes, `config.rs` handles persistence, `model.rs` retains only role resolution, new `extension_settings.rs` handles the config subcommands |
| **OCP / DIP** | Extensions declare their own settings via WIT; host doesn't hardcode knowledge of any extension's keys |
| **YAGNI** | No migration layer for old config.toml — greenfield, just change the format |
| **Value Objects** | Setting keys remain strings (the extension owns semantics); setting values are typed via `setting-schema` |

## Acceptance Criteria

### WIT changes

- [x] `setting-descriptor` gains `secret: bool` and `readonly: bool` fields
- [x] `model-descriptor` drops `settings`, `context_window_in`, `context_window_out`, `knowledge_cutoff`, `cost_in`, `cost_out` — retains only `id`, `name`, `description`, `is_default`
- [x] New `list-settings: func() -> list<setting-descriptor>` added to the base `extension` interface
- [x] WIT package version bumped to `0.3.0`

### Extension updates

- [x] `llm-google` implements `list-settings()` returning: `api_key` (secret), per-model dotted settings (mutable), per-model metadata as readonly settings
- [x] `llm-openrouter` implements `list-settings()` with same pattern, handling slash-containing model IDs (e.g. `openai/gpt-5.context_window_in`)
- [x] `session-jsonl` implements `list-settings()` returning `[]`
- [x] `compaction-llm` implements `list-settings()` returning `[]`
- [x] `test-extension` implements `list-settings()` returning `[]`
- [x] All extensions compile against updated WIT with `secret: false, readonly: false` defaults on existing setting-descriptors

### Config restructure

- [x] `UserConfig` struct changes `providers` field to `extensions: BTreeMap<String, BTreeMap<String, toml::Value>>` — flat map from extension ID to dotted-key settings
- [x] `settings_for()` reads from `[extensions.<provider>]` using dotted keys, validated against the `list-settings()` schema
- [x] `parse_model_ref()` remains unchanged (used by role resolution)
- [x] Old `[providers.*]` sections no longer recognized

### CLI restructure

- [x] `Command::Extensions` renamed to `Command::Extension` (singular)
- [x] `ExtensionAction` gains `Config` variant with nested `ConfigAction` subcommand
- [x] `ConfigAction` enum: `List { pattern: Option<String> }`, `Get { key: String }`, `Set { key: String, value: Option<String> }`
- [x] New `Command::Role` with `RoleAction`: `List`, `Get { role }`, `Set { role, model_ref }`
- [x] `Command::Model` removed entirely
- [x] `Command::Config` removed entirely
- [x] `handle_config()` in `main.rs` removed

### Config subcommand behavior

- [x] `config list`: queries `list-settings()`, merges with stored values from config.toml and keyring, prints table with columns KEY, TYPE, VALUE; secrets display as `****`; readonly values annotated with `(readonly)`
- [x] `config list <pattern>`: filters output by glob match on key
- [x] `config get <key>`: prints single value; secrets display as `****`; returns exit code 1 if key unknown
- [x] `config set <key> <value>`: validates against schema, writes to config.toml under `[extensions.<id>]`
- [x] `config set <key>` (no value, secret key): prompts without echo via `rpassword`, stores in keyring
- [x] `config set <key>` on readonly key: prints error, exits non-zero
- [x] `config set <key> <value>` on secret key: accepts inline value (no prompt), stores in keyring

### Role subcommand behavior

- [x] `role list`: prints table of configured roles with resolved provider/model
- [x] `role get <role>`: prints the provider/model for a role, with fallback chain info
- [x] `role set <role> <provider/model>`: validates model exists via `list-models()`, writes to `[roles]` in config.toml

### Integration

- [x] `turn::run()` uses `list-settings()` + config.toml to build `ConfigSetting` entries for provider `complete()` calls
- [x] Smoke tests pass with the new CLI shape
- [x] `make verify` passes (check, test, clippy, fmt)

## Implementation Phases

### Phase 1: WIT schema + bindings

Modify `wit/world.wit`: add `secret`/`readonly` to `setting-descriptor`, slim `model-descriptor`, add `list-settings()` to extension interface. Bump to `0.3.0`. Regenerate bindings. Update `extension_host.rs` to expose new `list_settings()` method. Estimated scope: 2 files.

### Phase 2: Extension updates

Update all 5 extensions (google, openrouter, session-jsonl, compaction-llm, test-extension) to implement `list-settings()`. LLM extensions declare their full settings namespace; others return `[]`. Update existing `setting-descriptor` constructions to include `secret: false, readonly: false`. Estimated scope: 5 extension `lib.rs` files.

### Phase 3: Config.toml restructure

Change `UserConfig` from `providers: BTreeMap<String, BTreeMap<String, BTreeMap<String, toml::Value>>>` to `extensions: BTreeMap<String, BTreeMap<String, toml::Value>>`. Update `settings_for()` to read dotted keys from the flat map. Update all call sites. Estimated scope: `config.rs`, `model.rs`, `turn.rs`.

### Phase 4: CLI commands

Restructure `cli.rs`: rename `Extensions` to `Extension`, add `Config` subgroup to `ExtensionAction`, add `Role` command, remove `Model` and top-level `Config`. Implement dispatch in `main.rs`. New module `extension_settings.rs` for `config list/get/set` logic (queries extension, merges config.toml + keyring, handles secret/readonly). Estimated scope: `cli.rs`, `main.rs`, new `extension_settings.rs`, `model.rs` (trim to just `resolve_role` + `collect_provider_models`).

### Phase 5: Cleanup + tests

Remove dead code from `model.rs` (old `cmd_*` functions). Update existing unit tests in `config.rs` and `model.rs`. Add tests for glob filtering, secret handling, readonly rejection. Run `make verify` and smoke tests. Estimated scope: `config.rs` tests, `model.rs` tests, new `extension_settings.rs` tests.

## Dependencies & Risks

- **`glob` crate**: needed for pattern matching in `config list`. Add via `cargo add glob`.
- **Dynamic catalogs**: OpenRouter's `list-settings()` requires a successful `init()` with API key. If no API key is configured, `config list` should still work — show extension-level settings (including `api_key` as secret/unset) but skip model-specific settings. The extension can return a partial list.
- **WASM recompilation**: changing WIT triggers recompilation of all extensions. The wasmtime cache mitigates this after first build.
- **Config.toml format change**: no migration needed (greenfield). Existing config.toml files from development will need manual update or deletion.

## References & Research

### Internal References

- Current CLI: [cli.rs](src/cli.rs)
- Current config: [config.rs](src/config.rs)
- Current model commands: [model.rs](src/model.rs)
- WIT definitions: [world.wit](wit/world.wit)
- Extension host: [extension_host.rs](src/extension_host.rs)
- Keyring: [keyring.rs](src/keyring.rs)
- Command dispatch: [main.rs](src/main.rs)
- Google extension: [extensions/system/llm-google/src/lib.rs](extensions/system/llm-google/src/lib.rs)
- OpenRouter extension: [extensions/system/llm-openrouter/src/lib.rs](extensions/system/llm-openrouter/src/lib.rs)

### Brainstorm

- [CLI Unified Extension Settings Brainstorm](.agents/brainstorms/2026-03-23-08-30-41-cli-unified-extension-settings-brainstorm.md)

### Prior Plans (foundations this builds on)

- [Extension Conflicts and Management](.agents/plans/2026-03-21-15-16-25-feat-extension-conflicts-and-management-plan.md)
- [Multi-Model Roles and Provider Capabilities](.agents/plans/2026-03-21-21-03-05-feat-multi-model-roles-and-provider-capabilities-plan.md)
- [Keyring API Key Storage](.agents/plans/2026-03-22-20-38-26-feat-keyring-api-key-storage-plan.md)
- [OpenRouter Provider](.agents/plans/2026-03-22-20-29-56-feat-openrouter-provider-and-dynamic-catalog-plan.md)
