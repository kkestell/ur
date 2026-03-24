---
title: "fix: Unified session event log with actual persistence and capability enforcement"
type: fix
date: 2026-03-23
supersedes: 2026-03-23-22-13-58-fix-unified-session-event-log-plan.md
---

# Unified Session Event Log + Extension Capability Declarations

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

**Capability gap:** The extension system currently determines WASI capabilities
by slot type (LLM extensions get HTTP, others don't). There is no mechanism for
extensions to declare what they need, and no enforcement if capabilities are
used without declaration. The session persistence work requires giving
`session-jsonl` filesystem access — this should be governed by an explicit
declaration, not an implicit slot-based grant.

## Solution

One canonical event log. Messages derived from events. Events persisted to disk.
Extensions declare filesystem and network capabilities; the host enforces them.

### Extension capability declarations

Add a flags type and lifecycle function to the `extension` interface:

```wit
flags extension-capabilities {
    filesystem-read,
    filesystem-write,
    network,
}

interface extension {
    // ... existing functions ...
    declare-capabilities: func() -> extension-capabilities;
}
```

Extensions return the set of capabilities they require. The host enforces these
in two ways:

1. **Static validation at load time.** After calling `declare-capabilities()`,
   the host inspects the component's WASI imports (via `ComponentType`). If the
   component imports `wasi:filesystem/*` but didn't declare `filesystem-read` or
   `filesystem-write`, the host panics. Same for `wasi:http/*` without
   `network`. This catches mismatches early with a clear error message.

2. **Structural enforcement.** The host only links WASI capabilities that the
   extension declared. Filesystem interfaces are only linked if
   `filesystem-read` or `filesystem-write` is declared. HTTP is only linked if
   `network` is declared. If a component somehow imports an unlinked capability,
   instantiation fails.

This replaces the implicit slot-based model. `llm-extension-http` no longer
silently gets HTTP — the extension must declare `network`. `session-jsonl` must
declare `filesystem-read` and `filesystem-write` to access its data directory.

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

Five steps, each independently compilable and testable.

### Step 1: Capability declarations in WIT + host enforcement

**File: `wit/world.wit`**

Add to `interface types`:

```wit
flags extension-capabilities {
    filesystem-read,
    filesystem-write,
    network,
}
```

Add to `interface extension`:

```wit
/// Declare the WASI capabilities this extension requires.
///
/// The host validates these against the component's actual imports.
/// Using a capability without declaring it will panic.
declare-capabilities: func() -> extension-capabilities;
```

Update all worlds to import all WASI capabilities. This makes every world
"maximally permissive" at the WIT level — enforcement is at the host:

```wit
world llm-extension {
    import host;
    import wasi:http/outgoing-handler@0.2.6;
    import wasi:filesystem/types@0.2.6;
    import wasi:filesystem/preopens@0.2.6;
    export extension;
    export llm-provider;
}

/// llm-extension-http is removed — llm-extension now covers this case.
/// Extensions that need HTTP declare `network` in their capabilities.

world session-extension {
    import host;
    import wasi:http/outgoing-handler@0.2.6;
    import wasi:filesystem/types@0.2.6;
    import wasi:filesystem/preopens@0.2.6;
    export extension;
    export session-provider;
}

world compaction-extension {
    import host;
    import wasi:http/outgoing-handler@0.2.6;
    import wasi:filesystem/types@0.2.6;
    import wasi:filesystem/preopens@0.2.6;
    export extension;
    export compaction-provider;
}

world general-extension {
    import host;
    import wasi:http/outgoing-handler@0.2.6;
    import wasi:filesystem/types@0.2.6;
    import wasi:filesystem/preopens@0.2.6;
    export extension;
}
```

Remove `llm-extension-http` — it's redundant now that capability access is
declaration-driven, not world-driven. Bump package version to `0.4.0`.

**File: `src/extension_host.rs`**

Add capability checking to `ExtensionInstance::load()`:

```rust
/// Validates that declared capabilities match component imports.
///
/// Panics if the component imports WASI capabilities it didn't declare.
fn validate_capabilities(
    engine: &Engine,
    component: &Component,
    capabilities: &ExtensionCapabilities,
    ext_id: &str,
) {
    let ct = component.component_type();
    let has_fs_import = ct.imports(engine).any(|(name, _)| name.contains("wasi:filesystem"));
    let has_http_import = ct.imports(engine).any(|(name, _)| name.contains("wasi:http"));

    if has_fs_import && !capabilities.contains(ExtensionCapabilities::FILESYSTEM_READ | ExtensionCapabilities::FILESYSTEM_WRITE) {
        panic!(
            "extension '{ext_id}' imports wasi:filesystem but did not declare \
             filesystem-read or filesystem-write"
        );
    }
    if has_http_import && !capabilities.contains(ExtensionCapabilities::NETWORK) {
        panic!(
            "extension '{ext_id}' imports wasi:http but did not declare network"
        );
    }
}
```

Update `build_host_state()` to accept capability flags and a data directory:

```rust
fn build_host_state(
    capabilities: &ExtensionCapabilities,
    data_dir: Option<&Path>,
) -> HostState {
    let mut builder = WasiCtxBuilder::new();
    builder.inherit_stdio();

    if let Some(dir) = data_dir {
        if capabilities.contains(ExtensionCapabilities::FILESYSTEM_READ)
            || capabilities.contains(ExtensionCapabilities::FILESYSTEM_WRITE)
        {
            let read = capabilities.contains(ExtensionCapabilities::FILESYSTEM_READ);
            let write = capabilities.contains(ExtensionCapabilities::FILESYSTEM_WRITE);
            // Preopen data_dir with declared permissions
            builder.preopened_dir(dir, "/data", read, write);
        }
    }

    HostState {
        wasi_ctx: builder.build(),
        http_ctx: WasiHttpCtx::new(),
        resource_table: ResourceTable::new(),
    }
}
```

Update linker setup to conditionally add HTTP:

```rust
// In the load() match arms — same pattern for all slots:
let mut linker = Linker::new(engine);
wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;
if capabilities.contains(ExtensionCapabilities::NETWORK) {
    wasmtime_wasi_http::p2::add_only_http_to_linker_sync(&mut linker)?;
}
```

Remove `llm-extension-http` bindgen module. Unify on one world per slot.

Add `declare_capabilities()` method to `ExtensionInstance`:

```rust
pub fn declare_capabilities(&mut self) -> wasmtime::Result<ExtensionCapabilities> {
    match self {
        Self::Llm { store, bindings } => {
            bindings.ur_extension_extension().call_declare_capabilities(store)
        }
        // ... same for all variants
    }
}
```

**File: `src/discovery.rs`**

Update `DiscoveredExtension` to include capabilities:

```rust
pub struct DiscoveredExtension {
    pub id: String,
    pub name: String,
    pub slot: Option<String>,
    pub source: SourceTier,
    pub wasm_path: PathBuf,
    pub checksum: String,
    pub capabilities: ExtensionCapabilities,
}
```

Call `declare_capabilities()` during `load_discovered()`, after `id()` and
`name()`.

**File: `src/manifest.rs`**

Add capabilities to `ManifestEntry`:

```rust
pub struct ManifestEntry {
    pub id: String,
    pub name: String,
    pub slot: Option<String>,
    pub source: String,
    pub wasm_path: String,
    pub checksum: String,
    pub enabled: bool,
    pub capabilities: Vec<String>, // ["filesystem-read", "filesystem-write", "network"]
}
```

Stored as a list of string tags for JSON serialization simplicity.

**Update all existing extensions** to implement `declare-capabilities`:

- `llm-google`: returns `network` (needs HTTP for Gemini API)
- `llm-openrouter`: returns `network` (needs HTTP for OpenRouter API)
- `session-jsonl`: returns `filesystem-read | filesystem-write` (Step 3)
- `compaction-llm`: returns empty (no special capabilities)
- `test-extension`: returns empty (no special capabilities)

**Tests:**

- `validate_capabilities` panics on undeclared filesystem import
- `validate_capabilities` panics on undeclared network import
- `validate_capabilities` passes when declarations match imports
- `build_host_state` preopens directory only when filesystem declared
- Discovery includes capabilities in `DiscoveredExtension`
- Manifest round-trips capabilities correctly

### Step 2: Revise event model + unify UrSession

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

### Step 3: WIT event type + session provider interface

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
the payload type changes from `message` to `session-event`.

**File: `src/extension_host.rs`**

Update `ExtensionInstance` methods:
- `load_session()` returns `Vec<wit_types::SessionEvent>` (was `Vec<Message>`)
- `append_session()` takes `&wit_types::SessionEvent` (was `&Message`)
- Method signatures and match arms updated to match new WIT types

Update session extension loading:
- Accept a `data_dir: &Path` parameter
- Pass data dir via `build_host_state()` with filesystem capabilities
- Pass data dir as init config entry: `("data_dir", "/data")` (the preopened
  path, not the host-side absolute path)

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

### Step 4: Implement session-jsonl persistence

**File: `extensions/system/session-jsonl/Cargo.toml`**

Add dependency: `serde`, `serde_json` (for JSON serialization of events).

The WIT-generated types need serialization. Use `serde_json::to_string()` on a
local representation that mirrors the WIT types, then write as JSONL.

**File: `extensions/system/session-jsonl/src/lib.rs`**

Update `declare_capabilities()` to return `filesystem-read | filesystem-write`.

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
        // Read preopened /data dir, list *.jsonl files, return SessionInfo per file
    }
}
```

Storage format: one JSONL file per session at `/data/{session_id}.jsonl`
(where `/data` is the preopened directory mapped to the host-side sessions
directory).

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

### Step 5: Wire up + smoke test

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
| `wit/world.wit` | Add `extension-capabilities` flags, `declare-capabilities` func, `session-event` variant + supporting types, change session-provider to event-based, unify all worlds with full WASI imports, remove `llm-extension-http` |
| `src/extension_host.rs` | Add `validate_capabilities()`, update `build_host_state()` for capability-driven WASI config, conditional HTTP linking, add `declare_capabilities()` method, update session methods for event types, remove `llm-extension-http` bindgen |
| `src/discovery.rs` | Add capabilities to `DiscoveredExtension`, call `declare_capabilities()` during discovery |
| `src/manifest.rs` | Add capabilities to `ManifestEntry`, persist in manifest JSON |
| `src/session.rs` | Remove `messages` vec, unify on `events`, add `messages_for_llm()`, revise `PersistedEvent` |
| `src/workspace.rs` | Pass data dir info through to session creation |
| `extensions/system/llm-google/src/lib.rs` | Implement `declare-capabilities` returning `network`, update world from `llm-extension-http` to `llm-extension` |
| `extensions/system/llm-openrouter/src/lib.rs` | Implement `declare-capabilities` returning `network`, update world from `llm-extension-http` to `llm-extension` |
| `extensions/system/session-jsonl/Cargo.toml` | Add `serde`, `serde_json` deps |
| `extensions/system/session-jsonl/src/lib.rs` | Implement `declare-capabilities` returning `filesystem-read \| filesystem-write`, implement JSONL read/write/list |
| `extensions/system/compaction-llm/src/lib.rs` | Implement `declare-capabilities` returning empty |
| `extensions/workspace/test-extension/src/lib.rs` | Implement `declare-capabilities` returning empty |
| `src/main.rs` | No change (already uses `SessionEvent` callback) |
| `scripts/smoke_test/test_agent_turn.py` | Add session persistence verification |

## Key Decisions

1. **Explicit capability declarations over implicit slot-based grants.** Extensions
   declare `filesystem-read`, `filesystem-write`, and/or `network` via
   `declare-capabilities()`. The host validates at load time and panics on
   mismatch. This replaces the pattern where slot type determines capabilities
   (e.g., `llm-extension-http` silently gets HTTP).

2. **Static validation + structural enforcement.** The host inspects component
   imports against declarations at load time (panic on mismatch). It also only
   links declared capabilities (belt and suspenders). This means undeclared
   access fails loudly and early.

3. **Remove `llm-extension-http` world.** With capability declarations, there's
   no need for a separate world that adds HTTP. All worlds import all WASI
   interfaces. The host controls what's actually available based on declarations.
   This eliminates the world-proliferation problem (no need for
   `session-extension-fs`, `general-extension-http-fs`, etc.).

4. **Embed full `Message` in events.** `LlmCompletion` and `ToolResult` carry
   the complete `Message` object. This preserves `provider_metadata_json`,
   message part grouping, and everything else the LLM needs. The alternative
   (fine-grained fields) loses information and makes reconstruction fragile.

5. **WIT variant, not JSON blob.** The `session-event` type is a proper WIT
   variant, not a string containing JSON. This gives type safety across the
   WASM boundary and lets future session providers (SQLite, cloud-backed) work
   with structured data.

6. **JSONL, not JSON.** Append-only writes. No need to parse/rewrite the
   entire file to add an event. Crash-safe — partial writes lose at most one
   event.

## Rust Development Note

This plan involves writing new Rust code. Invoke `/krust` before
implementation to ensure compliance with Rust development guidelines.
