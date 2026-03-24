# CLI Integration Tests Brainstorm

## How Might We

How might we turn manual, markdown-based CLI runbooks into automated integration tests that run reliably and catch regressions automatically?

## Why This Approach

The current `tests/cli/*.md` runbooks suffer from three compounding problems: they don't run in CI, they're executed inconsistently by hand, and they drift out of sync with code changes. Automating them as Rust integration tests using `assert_cmd` gives us regression coverage on every `cargo test` while testing the real CLI binary end-to-end.

## Assumptions (Validated)

1. All runbook tests get automated — both deterministic (echo LLM) and live API (Google, OpenRouter)
2. Missing API keys = hard error, no silent skipping
3. Selective execution via cargo's built-in `--test` / `--skip` filtering — no feature flags or custom machinery
4. Tests invoke the compiled `ur` binary via `assert_cmd` (not library-level testing)
5. Each test is hermetic — temp workspace, no shared state, parallelizable
6. WASM extensions built automatically via `build.rs` so `cargo test` is self-contained
7. Existing markdown runbooks are deleted after automation
8. Greenfield — no backwards compatibility concerns

## Constraints

- **Hermetic tests**: Each test creates its own temp workspace; no shared mutable state between tests
- **WASM build dependency**: Extensions are separate WASM compilation targets, not built by a normal `cargo build`. A `build.rs` will handle this automatically so `cargo test` just works.
- **API keys**: Loaded from `.env` at project root. Missing key = hard failure with clear error message.

## Key Decisions

### 1. Test style: assert_cmd binary tests
Tests invoke the `ur` binary end-to-end (arg parsing, output formatting, exit codes). Library-level integration tests (UrApp/UrWorkspace/UrSession) are a separate future effort.

### 2. File layout: by command, providers separate
```
tests/
  cli/
    mod.rs            # Shared helpers: temp_workspace, ur cmd builder, api_key loader
    extension.rs      # ur extension {list, enable, disable, inspect, config}
    role.rs           # ur role {list, get, set}
    run.rs            # ur run with echo LLM (deterministic)
    google.rs         # ur run with Google Gemini (live API)
    openrouter.rs     # ur run with OpenRouter (live API)
```

Provider tests are separate files because they have a distinct characteristic (API key requirement) even though they test the same command (`ur run`).

### 3. Helper design: thin composable functions
Small utility functions in `mod.rs` — no builders, no custom harness, no macros. Each test explicitly assembles its own setup. Verbose but transparent.

Key helpers:
- `temp_workspace() -> TempDir` — creates isolated workspace
- `ur(workspace: &Path) -> Command` — returns assert_cmd Command pointing at cargo-built binary with `-w` set
- `api_key(name: &str) -> String` — loads from `.env`, panics if missing
- `install_workspace_ext(ws: &Path, name: &str)` — copies a workspace extension into the temp workspace

### 4. WASM build: build.rs
A `build.rs` that compiles WASM extensions so `cargo test` is fully self-contained. Implementation details (caching, recursive cargo handling) to be worked out in planning.

### 5. Filtering: cargo built-in
```sh
cargo test --test cli                              # all CLI integration tests
cargo test --test cli extension                    # just extension tests
cargo test --test cli google                       # just Google provider
cargo test --test cli -- --skip google --skip openrouter  # skip live API tests
```

## Failure Modes

- **build.rs complexity**: Recursive cargo invocations from build.rs can deadlock on target directory locks. May need to shell out to `cargo` with a separate target dir or use a pre-build step. If build.rs proves too complex, fallback is `make test` depending on `build-extensions`.
- **Flaky live API tests**: Network issues or rate limits could cause intermittent failures. Mitigation: clear error messages distinguishing "API key missing" from "API call failed".
- **Test parallelism conflicts**: If tests share UR_ROOT, extension enable/disable in one test could affect another. Mitigation: each test gets its own UR_ROOT via temp dir.

## Open Questions

- Exact `build.rs` implementation strategy (recursive cargo, separate target dir, or alternative)
- Whether `UR_ROOT` should point to a shared read-only fixture dir (for system extensions) or be fully isolated per test
- Assertion granularity for CLI output — exact string match vs. contains vs. JSON parsing
- Whether to add new dependencies (assert_cmd, predicates, tempfile) via `cargo add` or document them for the plan

## Runbooks to Port

| Runbook | Target file | Key scenarios |
|---------|------------|---------------|
| extensions.md | extension.rs | Discovery, enable/disable, slot constraints, inspect |
| model-settings.md | extension.rs | Config set/get/list, constraint validation |
| model-roles.md | role.rs | List roles, assign model, validation |
| agent-turn.md | run.rs | Echo LLM turns, tool calls, deterministic output |
| google-provider.md | google.rs | Live completions, tool use, streaming |
| openrouter-provider.md | openrouter.rs | Live completions, tool use |
