---
title: "refactor: Split smoke test into Python with full CLI coverage"
type: refactor
date: 2026-03-22
---

# refactor: Split smoke test into Python with full CLI coverage

## Overview

Replace `scripts/smoke-test.sh` with a Python-based smoke test suite split into separate modules per logical group. Remove all assertions — the tests exist to run and produce observable output, not to validate. Audit the CLI surface and add missing coverage.

## Current State

One monolithic `scripts/smoke-test.sh` (~232 lines bash) containing:
- Build step (host + 5 extensions)
- Temp directory setup + WASM artifact copying
- Extension management tests (list, inspect, enable/disable, slot protection)
- Model role tests (list, get, set, config, fallback)
- Provider setting tests (set integer, reject unknown/out-of-range/wrong-type)
- Deterministic agent turn test (with grep assertions)
- Real API integration test (with grep assertions)

## Proposed Structure

```
scripts/
  smoke_test/
    __init__.py
    harness.py            # SmokeHarness class: build, temp dir, run helpers
    test_extensions.py    # Extension management
    test_model_roles.py   # Model role mappings
    test_model_settings.py # Provider settings
    test_agent_turn.py    # Deterministic agent turn
    test_integration.py   # Real API integration
  smoke-test.py           # Entry point: builds harness, runs all test modules
```

Delete `scripts/smoke-test.sh` after the new suite is working.

### `harness.py` — SmokeHarness Class

```python
class SmokeHarness:
    """Shared state and helpers for smoke tests."""

    def __init__(self, root: Path):
        self.root = root
        self.ur = root / "target/debug/ur"
        self.tmpdir: Path          # set during __enter__
        self.ur_root: Path
        self.workspace: Path

    def __enter__(self) -> "SmokeHarness":
        # Load .env
        # Build host + extensions via cargo
        # Create temp dir, directory trees, copy WASM artifacts
        return self

    def __exit__(self, *exc):
        # Clean up temp dir
        ...

    def run(self, *args: str) -> None:
        """Print and execute: ur <args>. Prints stdout/stderr."""
        ...

    def run_err(self, *args: str) -> None:
        """Like run(), but expects non-zero exit. Prints output regardless."""
        ...

    def cat(self, path: Path) -> None:
        """Print file contents."""
        ...
```

Key details:
- Uses `tempfile.TemporaryDirectory` for cleanup (no trap needed)
- `run()` prints `$ ur <args>` then runs with `UR_ROOT` and `-w` workspace set
- `run_err()` same but catches `CalledProcessError` — prints output, continues
- Loads `.env` with a simple line parser (no third-party deps — stdlib only)
- Build step calls `cargo build` for host and each extension WASM target

### `smoke-test.py` — Entry Point

```python
#!/usr/bin/env python3
from smoke_test.harness import SmokeHarness
from smoke_test import (
    test_extensions,
    test_model_roles,
    test_model_settings,
    test_agent_turn,
    test_integration,
)

root = Path(__file__).resolve().parent.parent

with SmokeHarness(root) as h:
    for module in [
        test_extensions,
        test_model_roles,
        test_model_settings,
        test_agent_turn,
        test_integration,
    ]:
        print(f"\n═══ {module.__name__.split('.')[-1]} ═══")
        module.run(h)

print("\nAll smoke tests complete.")
```

Each test module exposes a single `run(h: SmokeHarness)` function.

### `test_extensions.py` — Extension Management

**Currently tested:**
- `extensions list`
- `extensions inspect session-jsonl`
- `extensions inspect llm-google`
- `extensions disable llm-google` (error: last llm-provider)
- `extensions disable compaction-llm` (error: last compaction-provider)
- `extensions disable session-jsonl` (error: last session-provider)
- `extensions enable test-extension`
- `extensions disable test-extension`
- `extensions list` (final state)

**Add coverage for:**
- `extensions inspect test-extension` — workspace extension inspection
- `extensions inspect compaction-llm` — compaction slot extension
- `extensions inspect nonexistent` — non-existent extension (error path)
- `extensions enable nonexistent` — non-existent extension (error path)
- `extensions disable nonexistent` — non-existent extension (error path)
- `extensions enable test-extension` twice — already-enabled (error or no-op path)
- `extensions enable llm-test` — second LLM provider (multi-provider state)
- `extensions list` — verify both LLM providers visible
- `extensions disable llm-test` — remove second LLM (allowed since llm-google remains)

### `test_model_roles.py` — Model Role Mappings

**Currently tested:**
- `model list` (no config file)
- `model get default` (resolves to google default)
- `model get fast` (unknown role falls back to default)
- `model config default`
- `model set default google/gemini-3-flash-preview`
- `model get default` (verify persistence)
- `model set fast google/gemini-3-pro-preview`
- `model list` (shows both roles)
- `model config default` / `model config fast`
- `model set default fake/nonexistent` (error: unknown provider)
- `model set default invalid-no-slash` (error: bad format)
- `model set default google/nonexistent-model` (error: unknown model)

**Add coverage for:**
- `model config` after switching providers — set default to a different model and re-check

This section is already comprehensive. Minor additions only.

### `test_model_settings.py` — Provider Settings

**Currently tested:**
- `model setting default temperature 150`
- `model setting fast max_output_tokens 4096`
- `model setting default nonexistent_key 42` (error: unknown key)
- `model setting default temperature 999` (error: out of range)
- `model setting default temperature abc` (error: wrong type)
- Print `config.toml` contents

**Add coverage for:**
- `model setting default max_output_tokens 2048` — second setting on same role
- `model setting default temperature 0` — boundary value (minimum)
- `model setting default temperature 200` — boundary value (maximum)
- `model config default` after setting values — settings reflected in config output
- Print `config.toml` at end to show accumulated state

### `test_agent_turn.py` — Deterministic Agent Turn

**Currently tested:**
- Enable test-extension + llm-test
- Set default role to test/echo
- Run full agent turn
- Grep assertions for turn lifecycle events (REMOVE)
- Disable llm-test, reset model

**Changes:**
- Remove all grep assertions — just print the output
- Keep the enable/set/run/disable sequence

### `test_integration.py` — Real API Integration

**Currently tested:**
- Run agent turn with real `GOOGLE_API_KEY`
- Grep assertions for early lifecycle events (REMOVE)

**Changes:**
- Remove grep assertions — just print output
- Skip entirely if `GOOGLE_API_KEY` is not set (print skip message)
- Catch errors gracefully — real API may fail, print output regardless

## Acceptance Criteria

- [x] `SmokeHarness` handles build, temp dir, WASM copy, and `run()`/`run_err()` helpers
- [x] `smoke-test.py` is the single entry point, iterates test modules in order
- [x] Each test module has a `run(h)` function covering one logical area
- [x] No assertions — no `assert`, no `exit(1)` on missing output
- [x] Error paths use `run_err()` (catches non-zero exit) instead of assertions
- [x] New coverage items from each section above are included
- [x] `make smoke-test` updated to call `python3 scripts/smoke-test.py`
- [x] Integration test skips gracefully when `GOOGLE_API_KEY` is unset
- [x] Stdlib only — no third-party Python dependencies
- [x] Delete `scripts/smoke-test.sh` after new suite is verified

## References

- Current smoke test: `scripts/smoke-test.sh`
- CLI definition: `src/cli.rs`
- Extension implementations: `extensions/*/src/lib.rs`
- Slot definitions: `src/slot.rs`
- Turn orchestration: `src/turn.rs`
