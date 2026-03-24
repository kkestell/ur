---
title: "fix: Unified session event log with actual persistence"
type: fix
date: 2026-03-23
---

# Unified Session Event Log

## Problem

The plan for App/Workspace/Session called for a single session log that serves
both LLM context and UI replay, with filtering to extract what the LLM needs.
The implementation has two problems:

1. **Two separate vectors.** `UrSession` tracks `messages: Vec<Message>` and
   `events: Vec<PersistedEvent>` independently. They contain overlapping data
   (user messages, assistant messages, tool calls, tool results all appear in
   both). This is the opposite of "single source of truth."

2. **Nothing persists.** The `session-jsonl` extension is a stub — `append()`
   is a no-op, `load()` returns empty. The `events` vector is memory-only and
   vanishes when the process exits. Zero session files are written to disk.

Additionally, the current `PersistedEvent` model loses information during
reconstruction. When the LLM returns text + tool calls in one completion, only
`ToolCallRequested` events are emitted — the text is discarded, and the
grouping of parts within a single message is lost.

## Solution

One canonical event log. Messages derived from events. Events persisted to disk.

### Revised event model

Replace the fine-grained `PersistedEvent` variants with events that embed full
`Message` objects where needed, so message reconstruction is lossless:

```rust
pub enum PersistedEvent {
    TurnStarted { turn_index: u32 },
    UserMessage { text: String },
    /// Full LLM completion message (text + tool calls + provider metadata).
    LlmCompletion { message: wit_types::Message },
    /// Full tool result message.
    ToolResult { message: wit_types::Message },
    ToolApprovalRequested { id: String, name: String },
    ToolApprovalDecided { id: String, decision: ApprovalDecision },
    TurnComplete { turn_index: u32 },
    TurnInterrupted { turn_index: u32, reason: String },
}
```

Key change: `LlmCompletion` and `ToolResult` carry the full `Message` object.
No information loss. `provider_metadata_json` on tool calls is preserved for
round-tripping.

### Messages derived from events

```rust
fn messages_for_llm(events: &[PersistedEvent]) -> Vec<wit_types::Message> {
    events.iter().filter_map(|e| match e {
        PersistedEvent::UserMessage { text } => Some(user_message(text)),
        PersistedEvent::LlmCompletion { message } => Some(message.clone()),
        PersistedEvent::ToolResult { message } => Some(message.clone()),
        _ => None, // TurnStarted, approvals, etc. filtered out
    }).collect()
}
```

Three variants produce messages; everything else is UI/domain metadata that the
LLM never sees. This is the "filter" the original plan called for.

### UI replay from events

```rust
fn replay(&self, on_event: impl FnMut(SessionEvent)) {
    for event in &self.events {
        match event {
            PersistedEvent::LlmCompletion { message } => {
                // Extract tool calls or text for UI rendering
            }
            PersistedEvent::ToolResult { message } => {
                // Extract tool results for UI rendering
            }
            PersistedEvent::TurnComplete { .. } => {
                on_event(SessionEvent::TurnComplete);
            }
            // etc.
        }
    }
}
```

Same event log, different projections. One source of truth.

## Technical Approach

Four steps, each independently compilable and testable.

### Step 1: Revise event model + unify UrSession

**File: `src/session.rs`**

Remove from `UrSession`:
- `messages: Vec<wit_types::Message>` — gone
- `loaded_message_count: usize` — gone

Keep:
- `events: Vec<PersistedEvent>` — the single source of truth
- `turn_count: u32`

Revise `PersistedEvent` to the model above (embed `Message` in
`LlmCompletion` and `ToolResult`).

Add:
- `fn messages_for_llm(&self) -> Vec<wit_types::Message>` — derives messages
  from events for LLM calls
- `fn pending_events(&self) -> &[PersistedEvent]` — events since last persist

Update `run_turn()`:
- Push `PersistedEvent::UserMessage` instead of pushing to `self.messages`
- After LLM completion, push `PersistedEvent::LlmCompletion { message }`
- After tool dispatch, push `PersistedEvent::ToolResult { message }` per result
- Call `self.messages_for_llm()` when passing messages to LLM and compaction
- Remove `pending_session_appends()` — replaced by `pending_events()`

Update `snapshot()`:
- `messages` field derived from `messages_for_llm()`
- `events` field is direct

Update `replay()`:
- Adapt to new event variants

Update `persist_and_compact()`:
- Append events (not messages) to session provider
- Track `persisted_event_count` instead of `loaded_message_count`

Update tests:
- `pending_session_appends` tests → rewrite for `messages_for_llm()`
- `replay_emits_matching_session_events` → adapt to new variants
- Add `messages_for_llm_round_trips_correctly` test
- Add `messages_for_llm_filters_non_message_events` test

### Step 2: WIT event type + session provider interface

**File: `wit/world.wit`**

Add to `interface types`:

```wit
/// Approval decision for tool calls.
variant approval-decision {
    approve,
    deny,
}

/// A persisted session event.
variant session-event {
    turn-started(u32),
    user-message(string),
    llm-completion(message),
    tool-result(message),
    tool-approval-requested(tool-approval-request),
    tool-approval-decided(tool-approval-decision-record),
    turn-complete(u32),
    turn-interrupted(turn-interruption),
}

record tool-approval-request {
    id: string,
    name: string,
}

record tool-approval-decision-record {
    id: string,
    decision: approval-decision,
}

record turn-interruption {
    turn-index: u32,
    reason: string,
}
```

Change `interface session-provider`:

```wit
interface session-provider {
    use types.{session-info, session-event};

    /// Load all events for a session.
    load: func(id: string) -> result<list<session-event>, string>;

    /// Append an event to a session.
    append: func(id: string, event: session-event) -> result<_, string>;

    /// List available sessions.
    list-sessions: func() -> result<list<session-info>, string>;
}
```

The interface shape is identical (`load`, `append`, `list-sessions`) — only
the payload type changes from `message` to `session-event`. Bump package
version to `0.4.0`.

Add WASI filesystem imports to `session-extension` world:

```wit
world session-extension {
    import host;
    import wasi:filesystem/types@0.2.6;
    import wasi:filesystem/preopens@0.2.6;
    export extension;
    export session-provider;
}
```

**File: `src/extension_host.rs`**

Update `ExtensionInstance` methods:
- `load_session()` returns `Vec<wit_types::SessionEvent>` (was `Vec<Message>`)
- `append_session()` takes `&wit_types::SessionEvent` (was `&Message`)
- Method signatures and match arms updated to match new WIT types

Update session extension loading (the `Some("session-provider")` arm):
- Accept a `data_dir: &Path` parameter
- Add preopened directory via `WasiCtxBuilder::new().inherit_stdio().preopened_dir(...)`
- Pass data dir as init config entry: `("data_dir", path_str)`

**File: `src/session.rs`**

Update `load_slot()` and `load_session_provider()`:
- Pass the sessions data directory when loading session extensions
- Data directory: `{ur_root}/workspaces/{workspace_hash}/sessions/`
- Create directory if it doesn't exist

Update `UrSession::open()`:
- `load_session()` now returns events, not messages
- Initialize `events` from loaded events
- No `messages` or `loaded_message_count` to set

Update `persist_and_compact()`:
- Call `append_session(id, &event)` for each new event
- Compaction still operates on derived messages (call `messages_for_llm()`)

**File: `src/workspace.rs`**

Pass `ur_root` and workspace path through to session creation so the data
directory can be computed.

Also update the `host` interface in WIT to match — `load-session` returns
`list<session-event>`, `append-session` takes `session-event`. (The host
interface routes to the session provider, so types must align.)

### Step 3: Implement session-jsonl persistence

**File: `extensions/system/session-jsonl/Cargo.toml`**

Add dependency: `serde`, `serde_json` (for JSON serialization of events).

The WIT-generated types need serialization. Use `serde_json::to_string()` on a
local representation that mirrors the WIT types, then write as JSONL.

**File: `extensions/system/session-jsonl/src/lib.rs`**

Implement actual persistence:

```rust
impl SessionGuest for SessionJsonl {
    fn load(id: String) -> Result<Vec<SessionEvent>, String> {
        let path = session_path(&id);
        if !path.exists() { return Ok(Vec::new()); }
        let file = File::open(&path).map_err(|e| e.to_string())?;
        let reader = BufReader::new(file);
        let mut events = Vec::new();
        for line in reader.lines() {
            let line = line.map_err(|e| e.to_string())?;
            let event: SessionEvent = deserialize_event(&line)?;
            events.push(event);
        }
        Ok(events)
    }

    fn append(id: String, event: SessionEvent) -> Result<(), String> {
        let path = session_path(&id);
        let mut file = OpenOptions::new()
            .create(true).append(true)
            .open(&path).map_err(|e| e.to_string())?;
        let json = serialize_event(&event)?;
        writeln!(file, "{json}").map_err(|e| e.to_string())?;
        Ok(())
    }

    fn list_sessions() -> Result<Vec<SessionInfo>, String> {
        // Read data_dir, list *.jsonl files, return SessionInfo per file
    }
}
```

Storage format: one JSONL file per session at `{preopened_dir}/{session_id}.jsonl`.
Each line is a JSON object with a `type` field discriminator and event-specific
fields.

Example session file after one turn with a tool call:

```jsonl
{"type":"turn_started","turn_index":0}
{"type":"user_message","text":"What's the weather in Paris?"}
{"type":"llm_completion","message":{"role":"assistant","parts":[{"tool_call":{"id":"call-1","name":"get_weather","arguments_json":"{\"location\":\"Paris\"}","provider_metadata_json":""}}]}}
{"type":"tool_result","message":{"role":"tool","parts":[{"tool_result":{"tool_call_id":"call-1","tool_name":"get_weather","content":"Paris: 12C, cloudy"}}]}}
{"type":"llm_completion","message":{"role":"assistant","parts":[{"text":"The weather in Paris is 12°C and cloudy."}]}}
{"type":"turn_complete","turn_index":0}
```

Serialization: define local serde-friendly structs that mirror the WIT types
(WIT-generated types don't derive Serialize/Deserialize). Convert between
WIT types and serde types at the boundary.

### Step 4: Wire up + smoke test

**File: `src/session.rs`**

- `UrSession::open()` loads events from session provider, populates `self.events`
- `persist_and_compact()` appends new events, runs compaction on derived messages
- Second invocation of `ur run` with same session ID picks up where it left off

**Smoke test updates:**

Update `scripts/smoke_test/test_agent_turn.py`:
- After running a turn, inspect the session file on disk
- Verify JSONL content has expected event types
- Run a second turn, verify events append correctly
- Verify `list-sessions` returns the session

**Verification:**

```
make verify && make smoke-test
```

Then the manual test from the conversation:
```bash
ur run "Hello"
# Inspect session file — should contain events
cat $UR_ROOT/workspaces/.../sessions/demo.jsonl
```

## Files Affected

| File | Change |
|---|---|
| `src/session.rs` | Remove `messages` vec, unify on `events`, add `messages_for_llm()`, revise `PersistedEvent` |
| `wit/world.wit` | Add `session-event` variant + supporting types, change session-provider to event-based, add WASI filesystem to session world |
| `src/extension_host.rs` | Update session methods for event types, add filesystem preopens for session extensions |
| `src/workspace.rs` | Pass data dir info through to session creation |
| `extensions/system/session-jsonl/Cargo.toml` | Add `serde`, `serde_json` deps |
| `extensions/system/session-jsonl/src/lib.rs` | Implement JSONL read/write/list |
| `src/main.rs` | No change (already uses `SessionEvent` callback) |
| `scripts/smoke_test/test_agent_turn.py` | Add session persistence verification |

## Key Decisions

1. **Embed full `Message` in events.** `LlmCompletion` and `ToolResult` carry
   the complete `Message` object. This preserves `provider_metadata_json`,
   message part grouping, and everything else the LLM needs. The alternative
   (fine-grained fields) loses information and makes reconstruction fragile.

2. **WIT variant, not JSON blob.** The `session-event` type is a proper WIT
   variant, not a string containing JSON. This gives type safety across the
   WASM boundary and lets future session providers (SQLite, cloud-backed) work
   with structured data.

3. **WASI filesystem for session extensions.** The `session-extension` world
   gains WASI filesystem imports. The host preopens a data directory. This is
   the minimal capability grant — session extensions can only access their
   designated directory.

4. **JSONL, not JSON.** Append-only writes. No need to parse/rewrite the
   entire file to add an event. Crash-safe — partial writes lose at most one
   event.

## Rust Development Note

This plan involves writing new Rust code. Invoke `/krust` before
implementation to ensure compliance with Rust development guidelines.
