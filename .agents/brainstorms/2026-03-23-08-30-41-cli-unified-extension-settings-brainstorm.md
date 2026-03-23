# CLI Redesign: Unified Extension Settings

**Date:** 2026-03-23

## What We're Building

A CLI redesign that treats all extension configuration uniformly. Instead of separate `ur config`, `ur model config`, and `ur model setting` commands, everything goes through `ur extension <id> config {list|get|set}`. LLM-provider-specific concepts (API keys, model-specific settings) are handled by the same mechanism via schema flags (`secret`, `readonly`) and dotted key namespacing.

The top-level `ur model` command is eliminated. Role management (`default`, `fast`) moves to a dedicated `ur role` noun.

## Why This Approach

- **Extensions are the fundamental unit.** LLM providers, session providers, compaction providers — they're all extensions. The settings mechanism should reflect that rather than privileging one slot.
- **The CLI is a test harness right now.** Zero migration cost. When TUI/GUI arrives, the unified model makes it trivial to render a settings screen for any extension.
- **Eliminates naming confusion.** The current `config` vs `model config` vs `model setting` distinction is confusing. One noun (`extension`), one subgroup (`config`), three verbs (`list`, `get`, `set`).

## New CLI Shape

```
ur role list                                        # show role → model mappings
ur role set <role> <provider/model>                 # assign a role
ur role get <role>                                  # query a role

ur extension list                                   # all discovered extensions
ur extension <id> enable                            # enable an extension
ur extension <id> disable                           # disable an extension
ur extension <id> inspect                           # show extension details
ur extension <id> config list [pattern]             # list settings (optional glob filter)
ur extension <id> config get <key>                  # read a setting value
ur extension <id> config set <key> [value]          # write a setting (prompts if secret)

ur run                                              # execute a single agent turn
```

### Examples

```bash
# API keys — secret settings, stored in keyring, prompted without echo
ur extension google config set api_key
> API key for google: ****
> Stored.

# Model-specific settings — dotted keys
ur extension google config set gemini-flash.thinking_level high
> google: gemini-flash.thinking_level = high

# Read-only model metadata — queryable like any setting
ur extension google config get gemini-flash.cost_in
> 0.50

# Full settings listing
ur extension google config list
> KEY                                TYPE       VALUE
> api_key                            secret     ****
> gemini-flash.thinking_level        enum       medium
> gemini-flash.context_window_in     integer    1048576 (readonly)
> gemini-flash.cost_in               number     0.50 (readonly)
> ...

# Wildcard filtering — glob patterns match against dotted keys
ur extension openrouter config list openai/*
> KEY                                TYPE       VALUE
> openai/gpt-5.thinking_level        enum       medium
> openai/gpt-5.temperature           number     1.0
> openai/gpt-5.context_window_in     integer    128000 (readonly)
> ...

ur extension google config list gemini-flash.*
> KEY                                TYPE       VALUE
> gemini-flash.thinking_level        enum       medium
> gemini-flash.context_window_in     integer    1048576 (readonly)
> gemini-flash.cost_in               number     0.50 (readonly)
> ...

ur extension google config list *.thinking_level
> KEY                                TYPE       VALUE
> gemini-flash.thinking_level        enum       medium
> gemini-pro.thinking_level          enum       off
```

## Key Decisions

1. **Roles stay as a UX indirection layer** but move to `ur role`. Users think in terms of "my default model" and "my fast model". Settings belong to the model, not the role. TUI/GUI will present "configure default model settings" but resolve to the concrete model under the hood.

2. **Fully unified extension settings.** One `config {list|get|set}` interface for all extensions. No special `ur provider` or `ur config` nouns. The `config` subgroup cleanly separates extension management (enable/disable/inspect) from extension configuration.

3. **API keys are secret settings.** Extensions declare settings with a `secret: bool` flag in the schema. The host handles secret settings differently: prompts without echo, stores in OS keyring instead of config.toml. From the user's perspective, it's just `ur extension <id> set api_key`.

4. **Model-specific settings use dotted keys.** `gemini-flash.thinking_level` rather than a `--model` flag or nested subresource. The extension owns its key namespace — the host just passes keys through.

5. **Model metadata is read-only settings.** Context window, costs, knowledge cutoff are settings with `readonly: bool` in the schema. Queryable via `get`, visible in `settings` listing, but `set` rejects them.

6. **`ur model` is eliminated.** Model info is covered by read-only extension settings. Role assignment is `ur role`. No remaining need for a `model` noun.

7. **`config` subgroup with glob filtering.** Settings operations live under `ur extension <id> config {list|get|set}`, separating management from configuration. `config list` accepts an optional glob pattern to filter by key (e.g. `gemini-flash.*`, `openai/*`, `*.thinking_level`). Model names may contain slashes (e.g. OpenRouter's `openai/gpt-5`), and the dot separates model name from setting name.

## WIT Schema Changes Required

The setting descriptor needs two new flags:

```wit
record setting-descriptor {
    key: string,
    name: string,
    description: string,
    schema: setting-schema,
    secret: bool,       // NEW: store in keyring, prompt without echo
    readonly: bool,     // NEW: reject set, display in listings
}
```

Extensions declare all settings (including secrets and read-only metadata) in their schema. The host inspects these flags to determine storage and mutability.

## Open Questions

- **~~Settings listing scope~~** — Resolved: `config list` shows everything by default; optional glob pattern filters results (e.g. `config list gemini-flash.*`, `config list openai/*`, `config list *.thinking_level`).
- **~~Dotted key discovery~~** — Resolved: `config list` shows all keys including dotted prefixes. No separate `models` subcommand needed.
- **~~Config.toml structure~~** — Resolved: `[extensions.google]` section with dotted keys as TOML keys, e.g. `"gemini-flash.thinking_level" = "high"`.
- **~~Extension-level vs model-level settings in WIT~~** — Resolved: flattened. Extensions declare a single flat list of all settings (including dotted model-scoped ones). The host doesn't need to understand the grouping.
