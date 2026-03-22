# Multi-Model Roles & Provider Capabilities

**Date:** 2026-03-21
**Status:** Complete

## What We're Building

A **named role** system for model selection, where extensions request a role (e.g., `default`, `fast`) and the host resolves it to a specific provider/model pair. Combined with **self-describing providers** that declare their available models and per-model settings through the WIT interface.

### Core Concepts

1. **Model Roles** — Named aliases (`default`, `fast`) that map to a concrete `provider/model` pair. Extensions request a role; the host resolves it. Unmapped roles fall back to `default`.

2. **Self-Describing Providers** — Each `llm-provider` extension exports a `list-models()` WIT method returning model descriptors with typed settings schemas (e.g., Anthropic declares `claude-sonnet-4` supports `thinking_budget: integer, 0..128000, default 4000`).

3. **Global User Config** — Role-to-model mappings live in `~/.ur/config.toml` (or equivalent). One mapping applies everywhere.

4. **Provider + Model Pairs** — Roles resolve to explicit `provider/model` strings (e.g., `anthropic/claude-sonnet-4`). No ambiguous model-only references.

### Initial Roles

- `default` — The primary, capable model
- `fast` — Cheap/quick model for research, summarization, etc.

New roles can be added over time without code changes.

## Why This Approach

**WIT-declared provider capabilities (Approach A)** was chosen over static TOML declarations or a hybrid approach because:

- **Single source of truth** — The provider code owns its model list and settings schema. No drift between TOML metadata and runtime behavior.
- **Interactive CLI discovery** — `ur model config default` can show available settings by calling `list-models()` on the resolved provider. No schema duplication.
- **Future-proof** — Providers could dynamically fetch available models from APIs. Static TOML can't do this.
- **Natural fit** — Providers are already loaded as WASM modules. Adding a method costs nothing.

### Rejected Alternatives

- **Static TOML model declarations** — Simpler but static. Can't compute model lists at runtime. TOML schema for typed settings gets verbose and awkward.
- **Hybrid TOML + WIT validation** — Two sources of truth. Settings schema in TOML but validation in WASM creates surface area for bugs.
- **Capability tags** (e.g., `{speed: fast, cost: low}`) — More expressive but harder to reason about. Named roles are simpler and sufficient.
- **Freeform string keys** — Maximum flexibility but no structure. Named roles with fallback-to-default gives flexibility without chaos.
- **Role hierarchy / fallback chains** — Over-engineered for now. Simple fallback to `default` covers the need.

## Key Decisions

1. **Extensions decide** which role they need — the user doesn't pick per-call. User configures the mappings; extensions express intent.
2. **Named roles**, not capability tags or freeform strings.
3. **Unmapped roles fall back to `default`** — no errors, no hierarchy.
4. **Global user config** — no per-workspace overrides (for now).
5. **Explicit provider/model pairs** — `anthropic/claude-sonnet-4`, not just `claude-sonnet-4`.
6. **Providers self-describe via WIT** — `list-models()` returns model descriptors with typed settings schemas.
7. **CLI commands:**
   - `ur model get <role>` — show what a role resolves to
   - `ur model set <role> <provider/model>` — map a role (validated against provider's declared models)
   - `ur model config <role>` — interactive discovery of available settings for the resolved model
8. **Settings vary per provider** — thinking/reasoning is not a universal abstraction. Each provider declares its own settings (Anthropic: `thinking_budget` integer, OpenAI: `reasoning_effort` enum, etc.). The host passes them through opaquely.
9. **Zero-config cold start** — Each provider's `list-models()` marks one model as its recommended default. On first run with no `~/.ur/config.toml`, the host picks the first available provider's default model for the `default` role. User can override later with `ur model set`. No interactive setup required, no error on missing config.

## Resolved Questions

1. **Settings storage** — Separate from role mappings. Roles are just pointers (`default = "anthropic/claude-sonnet-4"`). Provider-specific settings live under the provider/model (`[providers.anthropic.claude-sonnet-4] thinking_budget = 8000`). Settings belong to the provider/model, not the role — remapping a role would break inline settings.

2. **WIT schema for settings descriptors** — Typed descriptors using a WIT variant type. `setting-schema` is a variant with `integer(min, max, default)`, `enumeration(allowed-values, default)`, and `boolean(default)` cases. Each `model-descriptor` contains a list of `setting-descriptor` records (key, name, description, schema). This gives the CLI enough information for interactive discovery and validation.

3. **Config file format** — New `~/.ur/config.toml`, separate from the workspace manifest. The manifest tracks workspace state (extensions, sessions). Config tracks user preferences (roles, provider settings). Different concerns, different files.

4. **How does `complete-opts` change?** — Two separate types. Extensions call `host.complete(messages, role)` with just a role string. The host resolves the role and calls the provider's `complete(messages, model, settings)` with the fully resolved model name and settings. Extensions never see or care about models or settings — that's the user's business.

5. **Dangling roles** — Warn at startup if a configured role references a provider/model that doesn't exist. The role falls back to `default`. Also validate at `ur model set` time that the provider/model exists.

## Config File Example

```toml
# ~/.ur/config.toml

[roles]
default = "anthropic/claude-sonnet-4"
fast = "openai/gpt-4o-mini"

[providers.anthropic.claude-sonnet-4]
thinking_budget = 8000

[providers.openai.gpt-4o-mini]
# no settings overrides — using provider defaults
```
