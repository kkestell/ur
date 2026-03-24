---
title: "fix: Preserve Gemini stream state, session history, and tool-call correlation"
type: fix
date: 2026-03-22
---

# Preserve Gemini stream state, session history, and tool-call correlation

## Overview

The current Google Gemini + turn-loop path has three correctness regressions:

1. The SSE parser consumes a `data:` line before it knows the line/event is complete, so streamed text or tool-call deltas can be dropped when a JSON chunk is split across reads.
2. The turn loop only appends `messages.last()` to the session provider, so the next turn loses the user message, assistant tool-call message, and tool-result messages that produced the final reply.
3. Gemini request serialization drops `ToolResult.tool_call_id`, so repeated or parallel calls to the same tool cannot be correlated back to the original `functionCall`.

Found brainstorm from 2026-03-22: `.agents/brainstorms/2026-03-22-llm-google-streaming-brainstorm.md`. Using it as background context for this plan.

## Problem Statement / Motivation

These are not cosmetic bugs. They break the three guarantees this tracer-bullet is supposed to prove:

- Streaming must be lossless across arbitrary read boundaries.
- Session history must contain the full conversational trace required for the next turn.
- Tool-call metadata must round-trip end-to-end so providers can disambiguate repeated calls.

A partial SSE read can silently corrupt the current response. Missing session appends make subsequent turns stateless even when the live turn appeared to work. Missing `functionResponse.id` makes same-name tool calls ambiguous, which becomes a real bug as soon as Gemini emits repeated or parallel tool calls.

## Research Summary

### Local findings

- `extensions/system/llm-google/src/lib.rs:199` iterates `remaining.lines()`, which yields the unterminated trailing line too.
- `extensions/system/llm-google/src/lib.rs:221` advances `state.pos` before parse success is known.
- `extensions/system/llm-google/src/lib.rs:419` serializes `ToolResult` as `functionResponse { name, response }` and drops the call ID.
- `src/turn.rs:127` adds the new user message to in-memory history, but `src/turn.rs:201` only persists the final message.
- `extensions/system/session-jsonl/src/lib.rs:35` and `extensions/system/session-jsonl/src/lib.rs:39` are still stubbed, so the current smoke test cannot observe append order/count bugs.

### Prior repo intent

- `.agents/plans/2026-03-22-feat-llm-google-gemini-integration-plan.md` explicitly mapped tool results to `functionResponse { name, id, response }`.
- `.agents/plans/2026-03-22-feat-self-describing-extensions-and-multi-part-messages-plan.md` already treated “functionResponse with name and id” as an acceptance criterion.
- `.agents/plans/2026-03-22-feat-tool-discovery-and-agent-turn-test-plan.md` added deterministic tool-call coverage, but not split-SSE coverage or observable session persistence coverage.
- `.agents/plans/2026-03-22-feat-pure-logic-unit-tests-plan.md` established the repo pattern of adding focused pure-logic unit tests instead of relying only on smoke tests.

### Institutional learnings

- No `.agents/solutions/` directory exists in this repo today, so there are no prior solution notes to incorporate.

### External references

- Google Gemini function calling docs: `https://ai.google.dev/gemini-api/docs/function-calling`
- Google Gemini thought signatures docs: `https://ai.google.dev/gemini-api/docs/thought-signatures`

The docs and prior internal plans agree on the important contract: tool responses must preserve the original call ID, and parallel/repeated tool calls rely on that identity.

## Proposed Solution

Fix the regression where data or identity is currently being dropped:

1. Make SSE parsing event-oriented and boundary-safe.
2. Persist every new message generated during a turn, in order, instead of only the final assistant reply.
3. Serialize `ToolResult.tool_call_id` into Gemini `functionResponse.id`.
4. Add regression tests that reproduce the three failures without requiring a live API call.

Keep the scope narrow. This is a bug-fix plan, not a redesign of the streaming WIT interface or session system.

## SpecFlow Analysis

### Flow 1: Split SSE event across reads

**Given** the HTTP body ends mid-`data:` line or mid-JSON object  
**When** `CompletionStream.next()` is called  
**Then** the parser keeps the unread bytes buffered and does not advance `state.pos`  
**And** after a later read completes the blank-line-delimited SSE event, exactly one `CompletionChunk` is emitted  
**And** no text/tool delta is lost or duplicated

### Flow 2: Tool-call turn persisted to session

**Given** an existing session history is loaded  
**When** a turn adds a new user message, an assistant tool-call message, one or more tool-result messages, and a final assistant reply  
**Then** only the messages created in this turn are appended  
**And** they are appended in the same order they were added to `messages`  
**And** the next turn can reload the full trace that produced the answer

### Flow 3: Parallel or repeated same-name tool calls

**Given** Gemini returns two `functionCall`s with the same `name` but different `id`s  
**When** the host serializes `ToolResult` messages back to Gemini  
**Then** each `functionResponse` includes the matching `id`  
**And** Gemini can correlate each response to the correct original call

## Technical Approach

### Phase 1: Lock the regressions with tests

**Goal:** Reproduce each bug before changing behavior.

#### `extensions/system/llm-google/src/lib.rs`

- [x] Add pure helper tests for SSE parsing with:
- [x] A buffer ending mid-JSON after `data: `
- [x] A buffer ending mid-line with no trailing newline
- [x] Two complete SSE events in one buffer
- [x] Both `\n\n` and `\r\n\r\n` event separators
- [x] Add serializer tests proving `MessagePart::ToolResult` becomes `functionResponse` with both `name` and `id`
- [x] Add a repeated-tool-name test where two tool results share `tool_name` but not `tool_call_id`

#### `src/turn.rs`

- [x] Extract a small pure helper or narrowly scoped persistence helper so append planning can be tested without a real session provider
- [x] Add a no-tool-path test proving the current turn persists both the new user message and the assistant reply
- [x] Add a tool-call-path test proving the persisted messages include:
- [x] User message
- [x] First assistant tool-call message
- [x] Tool result message(s)
- [x] Final assistant message

### Phase 2: Make SSE parsing boundary-safe

**Goal:** Never consume bytes until a full SSE event exists.

#### `extensions/system/llm-google/src/lib.rs`

- [x] Replace the current `remaining.lines()` scan with event-oriented parsing over `state.buffer[state.pos..]`
- [x] Detect complete SSE events via a blank-line delimiter, handling both LF and CRLF forms
- [x] Parse only bytes inside a complete event, gather all `data:` lines, and join them per SSE rules before JSON parsing
- [x] Advance `state.pos` only after a complete event has been identified and accepted for consumption
- [x] Preserve partial trailing lines/events in the buffer for the next read instead of dropping them
- [x] Trim already-consumed prefix bytes when safe so long streams do not grow the buffer forever

**Scope guard:** Keep the current interface shape. If a fully complete event is malformed JSON, log or skip explicitly rather than silently treating a partial event as consumed.

### Phase 3: Persist the full turn delta to session

**Goal:** Session history after the turn equals prior history plus every new message from this turn.

#### `src/turn.rs`

- [x] Track the boundary between loaded session history and turn-created messages, or collect `pending_session_appends` as the turn runs
- [x] Append every new message to the session provider in order after the turn completes
- [x] Do not re-append messages that were loaded from the session at the start of the turn
- [x] Preserve assistant tool-call messages and tool-result messages in persisted history, not just the final assistant text
- [x] Keep the implementation generic to `Message`; do not add provider-specific logic to the turn coordinator

**Testing note:** Because `extensions/system/session-jsonl/src/lib.rs` is still a stub, the minimum reliable coverage for this fix is host-side unit tests. A smoke-test assertion can be added later once session persistence is observable end-to-end.

### Phase 4: Restore tool-call correlation in Gemini requests

**Goal:** Preserve WIT `tool_call_id` all the way to Gemini.

#### `extensions/system/llm-google/src/lib.rs`

- [x] Update `message_to_gemini()` so `MessagePart::ToolResult` serializes to `functionResponse { name, id, response }`
- [x] Set `functionResponse.id` from `ToolResult.tool_call_id`
- [x] Keep the current JSON fallback for non-JSON tool output content
- [x] Add regression coverage for same-name repeated tool calls and mixed text/tool message histories

## Engineering Quality

| Principle | Application |
|---|---|
| **SRP** | Keep SSE parsing fixes inside `llm-google`; keep session append sequencing inside a small `turn.rs` helper instead of mixing it into streaming/output code. |
| **OCP / DIP** | `turn.rs` should stay generic over `Message` persistence and not learn Gemini-specific wire details. |
| **YAGNI / KISS** | Do not redesign WIT, streaming resources, or session storage. Fix the consumption and serialization boundaries already present. |
| **Value Objects** | Treat `tool_call_id` as required domain identity and preserve it end-to-end instead of flattening it away at serialization time. |

## Acceptance Criteria

### Functional

- [x] A partial SSE `data:` line is not consumed or dropped when an event spans multiple reads
- [x] Two complete SSE events buffered together can be emitted one-by-one with no duplication
- [x] A no-tool turn appends both the new user message and the assistant reply to the session
- [x] A tool-call turn appends, in order: user message, assistant tool-call message, tool result message(s), final assistant reply
- [x] Gemini request serialization includes `functionResponse.id` populated from `ToolResult.tool_call_id`
- [x] Same-name repeated or parallel tool calls remain distinguishable via their IDs

### Quality Gates

- [x] New regression tests fail before the fix and pass after it
- [x] `make test` passes
- [x] `make check` passes
- [x] No WIT schema changes are required for this bug fix
- [x] No provider-specific behavior leaks into `src/turn.rs`

## Success Metrics

- Multi-turn conversations can reconstruct the exact message history that produced the prior answer
- Streamed Gemini output no longer loses deltas when the transport splits an SSE event across reads
- Tool results remain correlated even when the same tool is called more than once in the same response

## Dependencies & Risks

- `CompletionStream.next()` currently returns `option<completion-chunk>`, not `result<...>`, so malformed complete SSE events still have a constrained error path. This fix should prioritize partial-event safety first.
- The existing smoke test cannot prove session append correctness while `session-jsonl` is a stub. Host-side unit tests are the authoritative regression protection for now.
- Buffer trimming must preserve `state.pos` correctness; otherwise a parser fix could introduce duplicate or skipped events.
- Parallel tool calls must preserve message ordering: assistant tool-call message first, then tool results, then the follow-up assistant reply.

## References & Research

### Internal References

- `.agents/brainstorms/2026-03-22-llm-google-streaming-brainstorm.md`
- `.agents/plans/2026-03-22-feat-llm-google-gemini-integration-plan.md`
- `.agents/plans/2026-03-22-feat-self-describing-extensions-and-multi-part-messages-plan.md`
- `.agents/plans/2026-03-22-feat-tool-discovery-and-agent-turn-test-plan.md`
- `.agents/plans/2026-03-22-feat-pure-logic-unit-tests-plan.md`
- `extensions/system/llm-google/src/lib.rs:199`
- `extensions/system/llm-google/src/lib.rs:221`
- `extensions/system/llm-google/src/lib.rs:419`
- `src/turn.rs:127`
- `src/turn.rs:201`
- `extensions/system/session-jsonl/src/lib.rs:35`

### External References

- Google Gemini function calling: `https://ai.google.dev/gemini-api/docs/function-calling`
- Google Gemini thought signatures: `https://ai.google.dev/gemini-api/docs/thought-signatures`
