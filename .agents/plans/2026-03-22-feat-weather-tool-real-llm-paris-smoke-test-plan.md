---
title: "feat: Add weather tool and real LLM Paris smoke test"
type: feat
date: 2026-03-22
---

# Add weather tool and real LLM Paris smoke test

## Overview

The workspace `test-extension` currently exposes a single `greet` tool, while the real integration smoke test only runs `ur run` without steering the prompt or asserting the tool loop. This plan replaces that demo shape with a weather-focused tool and updates the real LLM smoke path so a smoke-test run can visibly show: user weather question -> `get_weather` tool call -> tool result -> second LLM pass -> final answer about whether to wear a coat.

## Problem Statement / Motivation

The current smoke coverage proves extension discovery and a deterministic tool loop, but it does not yet prove the user-visible behavior you asked for:

1. `test-extension` exposes `greet`, not `get_weather(location: string) -> string`, so the available tool does not match the scenario we want to demonstrate in the trace.
2. The real integration test in `scripts/smoke_test/test_integration.py:8` only calls `ur run` and does not enable `test-extension`, set up a weather prompt, or verify that the LLM made a tool call and used its result.
3. `src/turn.rs:132` injects a hard-coded greeting prompt, so there is no test-specific way to ask about Paris weather and whether to wear a coat.

## Research Summary

### Local repo findings

- `extensions/workspace/test-extension/src/lib.rs:16` only handles `greet`, and `list_tools()` at `extensions/workspace/test-extension/src/lib.rs:31` only advertises `greet`.
- `scripts/smoke_test/test_extensions.py:21` only exercises extension management commands for `test-extension`; it does not cover an agent turn.
- `scripts/smoke_test/test_agent_turn.py:8` already provides deterministic end-to-end tool-loop coverage with `llm-test`, so this should remain as the non-network safety net.
- `scripts/smoke_test/test_integration.py:8` is the correct place to add the real Gemini weather scenario because it already gates on `GOOGLE_API_KEY`.
- `src/turn.rs:133` currently creates the user message inline, which is the main blocker for asking the Paris/coat question from smoke tests.
- `src/turn.rs:177`, `src/turn.rs:190`, and `src/turn.rs:193` already print the exact debug lines we want to see during a successful tool round trip.
- `scripts/smoke_test/harness.py:178` routes every `ur` invocation through a shared helper, which is the natural place to support a one-off environment override if `run` becomes prompt-configurable through an env var.

### Institutional learnings

No matching files were found under `.agents/solutions/`.

### External research decision

Proceed without external research. This is a local extension/test wiring task, and the repo already contains the relevant patterns for tool declaration, smoke harness execution, and real-provider integration.

## Proposed Solution

Implement the change in three small slices:

1. Replace the test extension's demo tool with `get_weather`, exposing a required `location` string parameter and returning a single hard-coded forecast line.
2. Make the tracer-bullet `run` prompt injectable for smoke tests so the real integration test can ask, "What is the weather in Paris, and should I wear a coat?" without permanently hard-coding that prompt for every run.
3. Upgrade the real integration smoke test to enable `test-extension`, run the weather prompt against Gemini, and assert that stdout shows the tool call, tool result, second LLM loop, and final answer.

## Technical Considerations

### 1. Test extension contract

Update `extensions/workspace/test-extension/src/lib.rs` so `call_tool()` matches on `get_weather` instead of `greet`, and `list_tools()` advertises:

- `name: "get_weather"`
- description explaining it returns a forecast for a location
- JSON schema requiring `location: string`

The returned value should stay intentionally simple: a single hard-coded line such as a cool/cloudy forecast that makes the coat recommendation plausible.

### 2. Prompt injection for `ur run`

`src/turn.rs:133` currently inlines `"Hello, please greet the world"`. For this feature, the smoke suite needs a way to provide a scenario-specific prompt without destabilizing unrelated CLI behavior.

Recommended approach:

- Add a small helper in `src/turn.rs` that reads an optional env var such as `UR_RUN_USER_MESSAGE`
- Fall back to the existing hard-coded prompt when the env var is absent
- Log the injected message through the existing `[turn] adding user message:` output

Why this is the best fit:

- It keeps the CLI surface unchanged
- It lets smoke tests steer the scenario precisely
- It avoids baking the Paris weather prompt into every manual `ur run`

### 3. Smoke harness support

If prompt override is env-based, add the smallest possible helper in `scripts/smoke_test/harness.py` so `test_integration.py` can run one command with a temporary environment override instead of mutating global harness state for all later commands.

Possible shapes:

- `run_with_env(env_overrides, *args)`
- or an optional `env` parameter on `run()` / `run_allow_error()`

Keep the change narrow and readable; no generalized harness abstraction is needed beyond this smoke-test use case.

### 4. Real integration test behavior

Update `scripts/smoke_test/test_integration.py` to:

- continue skipping when `GOOGLE_API_KEY` is unset
- enable `test-extension`
- ensure the default role resolves to `google/gemini-3-flash-preview`
- invoke `ur run` with the Paris/coat prompt override
- capture stdout and assert the trace contains:
  - `LLM returned tool call: get_weather(`
  - `tool result:`
  - `calling LLM streaming (... includes tool result)`
  - a final assistant answer that references Paris weather and/or whether to wear a coat
- disable `test-extension` during cleanup

### 5. Deterministic test compatibility

Changing the only test tool from `greet` to `get_weather` can ripple into `scripts/smoke_test/test_agent_turn.py:8` because the stub `llm-test` currently emits dummy arguments shaped like `{"name":"world"}`.

Keep the deterministic smoke test green by choosing one of these minimal fixes:

- update `extensions/workspace/llm-test/src/lib.rs` to emit `{"location":"Paris"}` when it calls `get_weather`
- or make `test-extension` tolerant of mismatched demo args while still advertising the correct schema

The first option is cleaner because it keeps the deterministic stub aligned with the published tool contract.

## Engineering Quality

| Principle | Application |
|-----------|-------------|
| **SRP** | `test-extension` remains a tiny demo tool provider; prompt injection stays inside the turn runner or smoke harness instead of leaking across modules. |
| **OCP / DIP** | An env-based prompt override extends `run` behavior without changing the CLI contract or adding a one-off command just for smoke tests. |
| **YAGNI / KISS** | Only add enough harness support to pass a prompt override for one command; avoid building a broad scenario framework. |
| **Value Objects** | No new domain value objects are needed; a raw `location` string is appropriate at this layer. |

Major component stereotypes:

- `extensions/workspace/test-extension/src/lib.rs`: Service Provider
- `src/turn.rs`: Coordinator
- `scripts/smoke_test/harness.py`: Test Harness / Coordinator
- `scripts/smoke_test/test_integration.py`: Scenario Test

## SpecFlow Analysis

Primary scenario:

1. Smoke test enables `test-extension`
2. Smoke test asks the real LLM about Paris weather and whether to wear a coat
3. Gemini sees `get_weather` in the tool list and emits a tool call
4. The host dispatches the tool and appends a `tool` message with the hard-coded forecast
5. The second LLM loop uses that tool result to answer the user
6. The smoke output clearly shows the call/result/final answer chain

Edge cases to cover:

- If `GOOGLE_API_KEY` is absent, the integration test still skips cleanly
- If Gemini chooses not to call the tool, the smoke test must fail loudly with the captured stdout so the regression is obvious
- If cleanup is skipped after a failure, later tests could inherit `test-extension` enabled state; use `try/finally`-style cleanup in the test implementation

## Acceptance Criteria

- [x] `extensions/workspace/test-extension/src/lib.rs` exposes `get_weather` with JSON schema requiring `location: string`
- [x] `get_weather` returns a single hard-coded forecast line suitable for a coat recommendation
- [x] `ur run` can accept a smoke-test-specific user message without removing the current default behavior when no override is supplied
- [x] `scripts/smoke_test/test_integration.py` asks about Paris weather and whether to wear a coat when `GOOGLE_API_KEY` is set
- [x] The real integration smoke test asserts that stdout includes the `get_weather` tool call, the tool result, and the second LLM loop before the final answer
- [x] The deterministic smoke test path in `scripts/smoke_test/test_agent_turn.py` remains green after the tool contract change
- [x] `make smoke-test` or `python3 scripts/smoke-test.py` produces a visible end-to-end weather tool trace when the Google API key is available
- [x] Tests are updated first or alongside implementation so the new behavior is defined by executable expectations

## Success Metrics

- A developer running the smoke suite with `GOOGLE_API_KEY` can visibly observe the LLM call `get_weather` and then answer using the returned forecast
- The smoke suite still passes without network credentials by skipping only the real integration segment
- The demo extension now reflects a realistic tool name and argument shape rather than a greeting placeholder

## Dependencies & Risks

- Real LLM behavior is probabilistic; prompt wording should strongly bias Gemini toward calling `get_weather`
- The hard-coded forecast should be phrased clearly enough that the model can infer a coat recommendation
- The integration test must clean up enabled extensions and model settings so later smoke steps are not polluted
- If env-driven prompt injection is implemented too broadly, it can become an undocumented testing backdoor; keep the interface small and obvious

## References & Research

- Similar implementation: `.agents/plans/2026-03-22-feat-tool-discovery-and-agent-turn-test-plan.md`
- Project guidance: `CLAUDE.md`
- Demo tool implementation: `extensions/workspace/test-extension/src/lib.rs:11`
- Deterministic tool-loop smoke test: `scripts/smoke_test/test_agent_turn.py:8`
- Real integration smoke test: `scripts/smoke_test/test_integration.py:8`
- Smoke harness command execution: `scripts/smoke_test/harness.py:178`
- Tracer-bullet user message and loop logging: `src/turn.rs:132`
