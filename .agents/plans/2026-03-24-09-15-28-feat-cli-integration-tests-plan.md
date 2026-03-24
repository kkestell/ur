---
title: "feat: Automated CLI integration tests"
type: feat
date: 2026-03-24
---

# Automated CLI Integration Tests

## Overview

Replace the manual markdown runbooks in `tests/cli/*.md` with automated Rust integration tests using `assert_cmd`. Tests invoke the compiled `ur` binary end-to-end, covering extension management, role configuration, echo LLM turns, and live API providers (Google, OpenRouter). Each test is hermetic — own temp workspace, own `UR_ROOT`, fully parallelizable.

## Acceptance Criteria

- [x] `make test` runs all CLI integration tests automatically
- [x] Extension tests cover discovery, enable/disable, slot constraints, inspect, and config
- [x] Role tests cover list, get, set, and validation errors
- [x] Run tests use echo LLM for deterministic turn verification
- [x] Google and OpenRouter tests exercise live API completions
- [x] Missing API key = hard failure with clear message (no silent skip)
- [x] All markdown runbooks in `tests/cli/` deleted
- [x] `cargo test --test cli` filters to just CLI tests; `--skip google --skip openrouter` excludes live API tests

## Architecture Decisions

### WASM build: Makefile dependency, not build.rs

The brainstorm proposed a `build.rs` to compile WASM extensions so `cargo test` is self-contained. Planning reveals this is the wrong tradeoff:

- **Recursive cargo deadlock**: `build.rs` runs inside cargo. Invoking `cargo build --target wasm32-wasip2` from within it creates a recursive cargo invocation that deadlocks on the target directory lock.
- **Separate target dir workaround**: Using a separate `--target-dir` avoids the lock but introduces cache misses, slow rebuilds, and a second build artifact tree to manage.
- **Makefile already solves this**: `make build-extensions` builds all WASM targets cleanly. Adding `test: build-extensions` makes `make test` self-contained with zero new complexity.

Decision: `make test` depends on `build-extensions`. Tests locate pre-built WASM files via `CARGO_MANIFEST_DIR` relative paths. Pure `cargo test` without prior `make build-extensions` will fail with a clear error ("WASM extension not found — run `make build-extensions` first").

### Hermetic UR_ROOT per test

Each test creates:
1. A temp dir for `UR_ROOT` — contains `extensions/system/`, `config.toml`, `workspaces/`
2. A temp dir for the workspace — passed via `-w`

The test helper copies pre-built system extension WASMs into the temp `UR_ROOT/extensions/system/` directory. This ensures no test modifies the real `~/.ur` and tests don't interfere with each other.

### Workspace extension installation

Workspace-tier test extensions (`llm-test`, `test-extension`) are copied into `{workspace}/.ur/extensions/{name}/` by the `install_workspace_ext` helper. The WASM files are located at their build output paths under `extensions/workspace/*/target/wasm32-wasip2/release/`.

## File Layout

```
tests/
  cli/
    mod.rs            # Shared helpers
    extension.rs      # ur extension {list, enable, disable, inspect, config}
    role.rs           # ur role {list, get, set}
    run.rs            # ur run with echo LLM (deterministic)
    google.rs         # ur run with Google Gemini (live API)
    openrouter.rs     # ur run with OpenRouter (live API)
  cli.rs              # Test harness entry point (mod declarations)
```

`tests/cli.rs` is the cargo integration test entry point:
```rust
mod cli;
```

Each submodule file contains `#[test]` functions. Cargo discovers them via `--test cli`.

## Helpers (`tests/cli/mod.rs`)

### `project_root() -> PathBuf`
Returns the project root via `CARGO_MANIFEST_DIR`.

### `wasm_path(tier: &str, name: &str) -> PathBuf`
Locates a pre-built WASM file: `{project_root}/extensions/{tier}/{name}/target/wasm32-wasip2/release/{name}.wasm` (with hyphens replaced by underscores in the filename). Panics with a helpful message if the file doesn't exist.

### `TestEnv`
Struct holding:
- `workspace: TempDir` — the `-w` workspace directory
- `ur_root: TempDir` — the `UR_ROOT` directory
- System extensions pre-installed

Constructor `TestEnv::new() -> TestEnv`:
1. Creates two temp dirs
2. Copies all four system extension WASMs into `{ur_root}/extensions/system/{name}/`
3. Returns the struct

### `TestEnv::ur(&self) -> Command`
Returns an `assert_cmd::Command` for the `ur` binary with:
- `-w {self.workspace.path()}`
- `env("UR_ROOT", self.ur_root.path())`
- `env_remove("HOME")` (prevent accidental config leakage)

### `TestEnv::install_workspace_ext(&self, name: &str)`
Copies workspace extension WASM into `{workspace}/.ur/extensions/{name}/`.

### `api_key(name: &str) -> String`
Loads `.env` from project root via `dotenvy`, reads the named key, panics if missing with: `"API key {name} not found in .env — see tests/cli/README.md"`.

## Steps

### Step 1 — Dependencies and Makefile

**Files:** `Cargo.toml` (via `cargo add`), `Makefile`

Add dev-dependencies:
```sh
cargo add --dev assert_cmd predicates tempfile dotenvy
```

Update Makefile:
1. Add `extensions/workspace/llm-test/Cargo.toml` to `REPO_EXTENSION_MANIFESTS` (it's currently missing — only `test-extension` is listed)
2. Add `llm-test` to the build-extensions loop by creating a new `TEST_EXTENSION_MANIFESTS` variable or adding it to `BUILTIN_EXTENSION_MANIFESTS` — but since it's not a "builtin" that ships with install, better to add a `REPO_EXTENSION_MANIFESTS` build loop that runs the subset needed for tests. Simpler: just add it to the build-extensions loop with a separate list:

```makefile
TEST_EXTENSION_MANIFESTS := \
    extensions/workspace/test-extension/Cargo.toml \
    extensions/workspace/llm-test/Cargo.toml

build-extensions:
    @for manifest in $(BUILTIN_EXTENSION_MANIFESTS) $(TEST_EXTENSION_MANIFESTS); do \
        ...
    done

test: build-extensions
    $(CARGO) test --manifest-path $(HOST_MANIFEST)
```

3. Add `test: build-extensions` dependency so `make test` is self-contained.

**Verify:** `make test` still passes (existing unit tests).

### Step 2 — Test harness and helpers

**Files:** `tests/cli.rs`, `tests/cli/mod.rs`

Create the test entry point and helper module as described in the Helpers section above.

Key implementation details:
- `TestEnv::new()` must create the full directory structure: `{ur_root}/extensions/system/{session-jsonl,compaction-llm,llm-google,llm-openrouter}/`
- Copy only the `.wasm` file for each extension (not the entire build tree)
- The `ur()` method uses `assert_cmd::Command::cargo_bin("ur")` to find the test-built binary

**Verify:** A trivial test (`ur().arg("--help").assert().success()`) passes via `cargo test --test cli`.

### Step 3 — Extension tests

**File:** `tests/cli/extension.rs`

Tests ported from `tests/cli/extensions.md` and `tests/cli/model-settings.md`:

**Discovery & listing:**
- [x] `list_shows_system_extensions` — `ur extension list` output contains all four system extensions with correct names and "system" source
- [x] `list_shows_enabled_status` — default-enabled extensions show "true", disabled show "false"

**Enable/disable:**
- [x] `enable_extension` — `ur extension enable <id>`, then `list` shows it enabled
- [x] `disable_extension` — `ur extension disable <id>`, then `list` shows it disabled
- [x] `enable_unknown_extension_fails` — nonexistent ID returns error exit code
- [x] `disable_violating_exactly_one_fails` — disabling the only session-provider fails with slot constraint error
- [x] `disable_violating_at_least_one_fails` — disabling all llm-providers fails

**Inspect:**
- [x] `inspect_shows_extension_details` — output includes id, name, slot, source, wasm_path, checksum

**Workspace extensions:**
- [x] `workspace_extension_discovered` — after `install_workspace_ext("test-extension")`, `ur extension list` shows it with "workspace" source
- [x] `workspace_extension_enable_disable` — can enable/disable workspace-tier extensions

**Config (settings):**
- [x] `config_list` — `ur extension config llm-google list` shows settings
- [x] `config_list_pattern` — `ur extension config llm-google list "gemini-3-flash*"` filters results
- [x] `config_get` — returns current value for a known setting
- [x] `config_set_enum` — set a valid enum value, verify with get
- [x] `config_set_invalid_enum_fails` — set an invalid enum value, expect error
- [x] `config_set_integer_bounds_fails` — set value outside bounds, expect error
- [x] `config_set_readonly_fails` — set a readonly field, expect error

### Step 4 — Role tests

**File:** `tests/cli/role.rs`

Tests ported from `tests/cli/model-roles.md`:

- [x] `role_list_shows_default` — `ur role list` output includes a "default" role
- [x] `role_get_default` — `ur role get default` returns a `provider/model` string
- [x] `role_set_and_get` — `ur role set myrole google/gemini-3-flash-preview`, then `ur role get myrole` returns it
- [x] `role_set_persists_to_config` — after set, `{ur_root}/config.toml` contains the role entry
- [x] `role_set_unknown_provider_fails` — `ur role set x bogus/model` fails
- [x] `role_set_malformed_ref_fails` — `ur role set x notaref` fails
- [x] `role_set_unknown_model_fails` — `ur role set x google/nonexistent` fails

### Step 5 — Run tests (echo LLM)

**File:** `tests/cli/run.rs`

Tests ported from `tests/cli/agent-turn.md`:

Setup: each test calls `install_workspace_ext("llm-test")` and `install_workspace_ext("test-extension")`, then enables both and sets `ur role set default test/echo`.

- [x] `echo_turn_returns_message` — `ur run "Hello"` exits successfully, stdout contains response text
- [x] `echo_turn_echoes_input` — the echo provider mirrors input; verify stdout contains the sent message
- [x] `verbose_shows_session_events` — `ur -v run "Hello"` stderr includes session event trace lines (TextDelta, AssistantMessage, etc.)
- [x] `tool_call_round_trip` — send a message that triggers the weather tool, verify ToolCall and ToolResult events appear in verbose output

### Step 6 — Google provider tests

**File:** `tests/cli/google.rs`

Tests ported from `tests/cli/google-provider.md`. Each test calls `api_key("GOOGLE_API_KEY")` at the start (panics if missing).

Setup: use default system extensions (llm-google is already enabled).

- [x] `google_flash_basic` — `ur run "Say hello"` with default role (google/gemini-3-flash-preview) returns non-empty output
- [x] `google_pro_model` — set role to `google/gemini-3.1-pro-preview`, run, verify non-empty output
- [x] `google_thinking_level` — set `gemini-3-flash-preview.thinking_level` to `"high"`, run, verify success
- [x] `google_max_output_tokens` — set `gemini-3-flash-preview.max_output_tokens`, run, verify success
- [x] `google_consecutive_runs` — run two messages sequentially, both succeed

### Step 7 — OpenRouter provider tests

**File:** `tests/cli/openrouter.rs`

Tests ported from `tests/cli/openrouter-provider.md`. Each test calls `api_key("OPENROUTER_API_KEY")`.

Setup: disable `llm-google` so `llm-openrouter` is the sole LLM provider (avoids provider ambiguity). Install `test-extension` for tool-calling tests.

- [x] `openrouter_basic` — `ur run "Say hello"` returns non-empty output
- [x] `openrouter_tool_use` — enable test-extension, send message that triggers weather tool, verify output references tool result

### Step 8 — Delete runbooks, final verification

**Files:** Delete `tests/cli/*.md` (all markdown runbooks including `README.md`)

- [x] `make verify` passes (fmt-check, check, test, clippy)
- [x] `cargo test --test cli` runs all tests
- [x] `cargo test --test cli -- --skip google --skip openrouter` runs only deterministic tests
- [x]No markdown runbooks remain in `tests/cli/`

## Risks

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| WASM files not built before `cargo test` | Medium | Clear panic message pointing to `make build-extensions`; `make test` handles it automatically |
| Live API tests flaky from network/rate limits | Medium | Tests are simple single-turn completions; clear error messages distinguish "key missing" from "API failed" |
| System extension WASM copy slow for many tests | Low | WASM files are small (~few MB total); copy is fast |
| Extension enable/disable state leaks between tests | Low | Each test gets its own `UR_ROOT` temp dir; no shared state |
