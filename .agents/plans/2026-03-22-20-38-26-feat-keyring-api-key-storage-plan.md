---
title: "feat: Add keyring-based API key storage for LLM providers"
type: feat
date: 2026-03-22
---

# feat: Add keyring-based API key storage for LLM providers

## Overview

Store one API key per LLM provider in the OS keyring via the `keyring` crate. Add a `ur config set-key <provider>` CLI command with interactive (hidden) input. At runtime, resolve API keys from keyring with env-var override, and panic with a clear message if no key is found.

## Problem Statement / Motivation

API keys are currently read from environment variables only (`turn.rs:254-265`). This requires users to manage `.env` files or shell exports, which is fragile, easy to leak into shell history, and doesn't persist across sessions without extra setup. The OS keyring (macOS Keychain, Linux secret-service/kwallet, Windows Credential Manager) is the standard secure credential store.

## Proposed Solution

### 1. Add `keyring` dependency

```
cargo add keyring
```

The `keyring` crate provides a cross-platform API over OS credential stores. It uses a **service name** + **username** pair to identify entries.

- **Service name:** `"ur"`
- **Username:** provider ID (e.g., `"google"`, `"anthropic"`, `"openai"`)

### 2. New module: `src/keyring.rs`

Thin wrapper around the `keyring` crate. Two public functions:

```rust
/// Store an API key for a provider, overwriting any existing value.
pub fn set_api_key(provider_id: &str, key: &str) -> Result<()>

/// Retrieve the API key for a provider, or None if not set.
pub fn get_api_key(provider_id: &str) -> Result<Option<String>>
```

Both use `keyring::Entry::new("ur", provider_id)` internally.

### 3. New CLI subcommand: `ur config set-key <provider>`

Add a `Config` variant to the `Command` enum in `cli.rs`:

```rust
/// Manage configuration.
Config {
    #[command(subcommand)]
    action: ConfigAction,
},
```

```rust
#[derive(Subcommand, Debug)]
pub enum ConfigAction {
    /// Set the API key for an LLM provider.
    SetKey {
        /// Provider ID (e.g. "google", "anthropic").
        provider: String,
    },
}
```

**Interactive input:** The handler reads the key from stdin with echo disabled using `rpassword::read_password()`. Add `rpassword` as a dependency.

```
cargo add rpassword
```

Handler flow:
1. Print `"API key for {provider}: "` (no newline) to stderr
2. Read hidden input via `rpassword::read_password()`
3. Trim whitespace
4. Validate non-empty
5. Call `keyring::set_api_key(provider, &key)`
6. Print `"API key stored for {provider}."`

### 4. Modify API key resolution in `turn.rs`

Replace the current `llm_init_config()` (lines 254-265) with a two-tier lookup:

```rust
fn llm_init_config(provider_id: &str) -> Vec<(String, String)> {
    // Convention: env var is {PROVIDER_ID_UPPER}_API_KEY
    let env_key = format!("{}_API_KEY", provider_id.to_uppercase());

    // 1. Env var wins (CI, scripting, temporary override)
    if let Ok(val) = std::env::var(&env_key) {
        return vec![("api_key".into(), val)];
    }

    // 2. Keyring
    match keyring::get_api_key(provider_id) {
        Ok(Some(val)) => return vec![("api_key".into(), val)],
        Ok(None) => {}
        Err(e) => eprintln!("warning: keyring lookup failed for {provider_id}: {e}"),
    }

    // 3. No key found — panic with actionable message
    panic!(
        "No API key for provider '{provider_id}'. \
         Set one with: ur config set-key {provider_id}"
    );
}
```

Key design decisions:
- **Env var takes precedence** over keyring. This preserves CI/scripting workflows and lets users temporarily override without touching the keyring.
- **Generic env var convention:** `{PROVIDER_ID_UPPER}_API_KEY` instead of a hardcoded match. Scales to any provider.
- **Panic on missing key** as requested. The message tells the user exactly what to do.

### 5. Wire up in `main.rs`

Add the new command arm:

```rust
Command::Config { action } => match action {
    ConfigAction::SetKey { provider } => {
        eprint!("API key for {provider}: ");
        let key = rpassword::read_password()?;
        let key = key.trim();
        anyhow::ensure!(!key.is_empty(), "API key cannot be empty");
        keyring::set_api_key(&provider, key)?;
        println!("API key stored for {provider}.");
    }
},
```

Note: The `Config` command doesn't need the wasmtime engine, manifest, or workspace. It can be handled before those are initialized, or the engine init can be moved below the match (lazy).

## Technical Considerations

- **Linux secret-service:** The `keyring` crate defaults to the D-Bus Secret Service API (GNOME Keyring, KDE Wallet). On headless servers without a keyring daemon, it will fail. The panic message is sufficient for now; future work could add a file-based fallback.
- **No validation of provider ID:** We don't check that the provider exists before storing a key. This is intentional — the user may set a key before installing the extension.
- **Engine initialization:** Currently `main.rs` creates the wasmtime engine before matching commands. The `config set-key` path doesn't need it. Consider moving engine creation into the branches that need it to avoid unnecessary startup cost. Alternatively, just leave it — it's fast with the cache.

## Acceptance Criteria

- [x] `keyring` and `rpassword` crates added as dependencies
- [x] `src/keyring.rs` module with `set_api_key` and `get_api_key` functions
- [x] `ur config set-key <provider>` command prompts for key with hidden input and stores in OS keyring
- [x] `llm_init_config()` resolves keys: env var > keyring > panic
- [x] Env var naming convention is generic: `{PROVIDER_ID_UPPER}_API_KEY`
- [x] Panic message includes the `ur config set-key` command to run
- [x] `make verify` passes (fmt, check, test, clippy — including WASM extension targets)

## Files to Touch

| File | Change |
|------|--------|
| `Cargo.toml` | Add `keyring`, `rpassword` deps (via `cargo add`) |
| `src/keyring.rs` | New module — `set_api_key`, `get_api_key` |
| `src/cli.rs` | Add `Config` command variant with `ConfigAction::SetKey` |
| `src/main.rs` | Add `mod keyring`, wire `Command::Config` handler, import `ConfigAction` |
| `src/turn.rs` | Replace `llm_init_config()` with keyring+env resolution + panic |

## References

- [keyring crate](https://crates.io/crates/keyring) — cross-platform credential storage
- [rpassword crate](https://crates.io/crates/rpassword) — hidden password input
- Current env-var handling: [turn.rs:254-265](src/turn.rs#L254-L265)
- CLI structure: [cli.rs:20-34](src/cli.rs#L20-L34)
- Extension init pattern: [extensions/system/llm-google/src/lib.rs:50-58](extensions/system/llm-google/src/lib.rs#L50-L58)
