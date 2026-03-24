---
title: "feat: Replace println debug output with structured tracing and add -v/--verbose flag"
type: feat
date: 2026-03-23
---

# feat: Structured Logging with --verbose Flag

## Overview

Replace all internal/debug `println!` calls with the `tracing` crate and gate log output behind a `-v`/`--verbose` CLI flag. Only actual agent output (streamed LLM text) and CLI command output (tables, confirmations) should print without the flag.

## Problem Statement

Every `println!` in the codebase goes unconditionally to stdout. Debug trace output from `turn.rs` (`[turn] ...` lines) mixes with user-facing agent responses and CLI table output. There's no way to silence internal logging or selectively enable it.

## Proposed Solution

1. Add `tracing` + `tracing-subscriber` dependencies
2. Add `-v`/`--verbose` flag to the `Cli` struct
3. Replace debug/warning prints with `tracing` macros (`info!`, `warn!`, `debug!`, `trace!`)
4. Keep `println!` only for user-facing CLI output and streamed agent text
5. Initialize a `tracing-subscriber` that writes to stdout only when `--verbose` is set

## Categorization of Current Print Statements

### Keep as `println!` (user-facing output)

These are CLI command output — tables, confirmations, streamed text:

| File | What | Why keep |
|------|------|----------|
| `src/cli.rs:96,107-113,119` | Extension list/inspect tables | CLI command output |
| `src/model.rs:109,112,115,122,143` | Role list/get/set output | CLI command output |
| `src/extension_settings.rs:72,106,130,132,139,150,187,202` | Config list/get/set output | CLI command output |
| `src/main.rs:54,60` | "Enabled/Disabled" confirmations | CLI command output |
| `src/turn.rs:59` | `print!("{delta}")` — streamed LLM text | Agent output to user |
| `src/turn.rs:86` | `println!()` — newline after stream | Agent output to user |
| `src/extension_settings.rs:178` | `eprint!` prompt for password | Interactive prompt |

### Convert to `tracing` macros

| File | Lines | Current | Target Level | Notes |
|------|-------|---------|-------------|-------|
| `src/turn.rs` | 139,148 | `[turn] loading/loaded session` | `info!` | Session lifecycle |
| `src/turn.rs` | 160 | `[turn] adding user message` | `debug!` | Message content |
| `src/turn.rs` | 172 | `[turn] collected N tools` | `info!` | Tool discovery |
| `src/turn.rs` | 180 | `[turn] resolving role` | `info!` | Role resolution |
| `src/turn.rs` | 184,214 | `[turn] calling LLM streaming` | `info!` | LLM calls |
| `src/turn.rs` | 193,219 | `[turn] LLM returned message` | `debug!` | LLM responses |
| `src/turn.rs` | 199 | `[turn] LLM returned tool call` | `info!` | Tool calls |
| `src/turn.rs` | 228 | `[turn] appending messages` | `debug!` | Session append |
| `src/turn.rs` | 240,248 | `[turn] compacting/result` | `info!` | Compaction |
| `src/turn.rs` | 258 | `[turn] done` | `info!` | Turn complete |
| `src/turn.rs` | 289 | `[turn] dispatching tool` | `info!` | Tool dispatch |
| `src/turn.rs` | 305 | `[turn] tool result` | `debug!` | Tool results |
| `src/model.rs` | 52 | `warning: init failed` | `warn!` | Provider init |
| `src/model.rs` | 62 | `warning: list-models failed` | `warn!` | Provider query |
| `src/provider.rs` | 17 | `warning: keyring lookup failed` | `warn!` | Keyring error |
| `src/extension_host.rs` | 107 | `[host log]` | `debug!` | Extension log callback |
| `extensions/.../llm-openrouter/src/lib.rs` | 128 | `eprintln! catalog fetch` | N/A — WASM guest, leave as-is | Cannot use tracing in WASM guest |

## Implementation

### Phase 1: Add dependencies and CLI flag

**`Cargo.toml`** — add via `cargo add`:
```
cargo add tracing
cargo add tracing-subscriber --features env-filter
```

**`src/cli.rs`** — add verbose flag to `Cli` struct:
```rust
#[derive(Parser, Debug)]
#[command(name = "ur")]
pub struct Cli {
    /// Workspace directory.
    #[arg(short, long)]
    pub workspace: Option<PathBuf>,

    /// Enable verbose logging output.
    #[arg(short, long, global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Command,
}
```

### Phase 2: Initialize tracing subscriber

**`src/main.rs`** — initialize subscriber before command dispatch:
```rust
use tracing_subscriber::EnvFilter;

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    if cli.verbose {
        tracing_subscriber::fmt()
            .with_env_filter(
                EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| EnvFilter::new("ur=debug")),
            )
            .with_target(true)
            .with_writer(std::io::stderr)
            .init();
    }

    // ... rest of main
}
```

Notes:
- Write logs to **stderr** so they don't mix with piped agent output on stdout
- Default filter `ur=debug` shows all host crate logs; `RUST_LOG` env var overrides for fine-tuning
- When `--verbose` is NOT set, no subscriber is installed → zero overhead, zero output

### Phase 3: Convert turn.rs debug prints

Replace all `[turn]` prefixed `println!` calls with structured tracing macros. Use `tracing::info_span!("turn")` for the turn execution context, and structured fields:

```rust
use tracing::{info, debug, info_span, Instrument};

// Example conversions:
// Before: println!("[turn] loading session \"{session_id}\"...");
// After:  info!(session_id, "loading session");

// Before: println!("[turn] collected {} tool{}...", tools.len(), ...);
// After:  info!(count = tools.len(), "collected tools");

// Before: println!("[turn] LLM returned tool call: {}({})", tc.name, tc.arguments_json);
// After:  info!(tool = tc.name, args = tc.arguments_json, "LLM returned tool call");
```

### Phase 4: Convert warning prints

Replace `eprintln!("warning: ...")` in `model.rs`, `provider.rs` with `tracing::warn!`:

```rust
// Before: eprintln!("warning: {}: init failed: {e}", entry.id);
// After:  tracing::warn!(extension = %entry.id, error = %e, "init failed");
```

### Phase 5: Convert extension host log

Replace `println!("[host log] {msg}")` in `extension_host.rs` with `tracing::debug!`:

```rust
// Before: println!("[host log] {msg}");
// After:  tracing::debug!(msg, "extension log");
```

### Out of scope: WASM guest extensions

The `eprintln!` in `extensions/system/llm-openrouter/src/lib.rs` runs inside a WASM guest — it cannot use the host's `tracing` subscriber. Leave as-is. (Future: route guest stderr through the host log callback, which already exists and will now use `tracing::debug!`.)

## Acceptance Criteria

- [x] `ur run` with no flags prints only streamed LLM text to stdout (no `[turn]` lines)
- [x] `ur -v run` prints structured log lines to stderr alongside LLM text on stdout
- [x] `ur extension list`, `ur role list`, etc. still print their tables to stdout unchanged
- [x] `RUST_LOG=ur::turn=trace ur -v run` allows fine-grained filtering
- [x] `make verify` passes (fmt, check, test, clippy)
- [x] `make smoke-test` passes
- [x] Warning messages (provider init failures, keyring errors) appear in verbose mode only — they are no longer unconditionally printed to stderr

## Files Modified

| File | Changes |
|------|---------|
| `Cargo.toml` | Add `tracing`, `tracing-subscriber` (via `cargo add`) |
| `src/main.rs` | Initialize tracing subscriber, thread `verbose` flag |
| `src/cli.rs` | Add `verbose: bool` to `Cli` struct |
| `src/turn.rs` | Replace ~18 `println!` calls with `tracing` macros |
| `src/model.rs` | Replace 2 `eprintln!` warnings with `tracing::warn!` |
| `src/provider.rs` | Replace 1 `eprintln!` warning with `tracing::warn!` |
| `src/extension_host.rs` | Replace 1 `println!` with `tracing::debug!` |

## References

- [Frontend separation brainstorm](../../.agents/brainstorms/2026-03-22-13-54-38-frontend-separation-and-core-api-brainstorm.md) — typed events model aligns with structured logging
- [ReAct loop hooks brainstorm](../../.agents/brainstorms/2026-03-23-16-15-50-react-loop-hooks-brainstorm.md) — hook observability points
- tracing crate: https://docs.rs/tracing
- tracing-subscriber: https://docs.rs/tracing-subscriber
