---
title: "feat: App/Workspace/Session core API"
type: feat
date: 2026-03-23
---

# App/Workspace/Session Core API

## Overview

Extract a headless core API from the current CLI-embedded orchestration. Three
new types — `UrApp`, `UrWorkspace`, `UrSession` — give clients a coherent,
structured interface to all Ur functionality. The CLI becomes a thin consumer of
this API rather than the de facto application layer.

## Problem Statement

Today `main.rs` directly coordinates manifest loading, provider discovery,
config updates, and turn execution. `turn.rs` is close to a runtime coordinator
but prints to stdout, owns streaming presentation, and is only callable from the
CLI binary. There is no way for a future TUI, GUI, or test harness to drive Ur
without reimplementing this orchestration.

Key files that need extraction:

| Current location | Concern |
|---|---|
| [main.rs:27-125](src/main.rs#L27-L125) | CLI parsing + engine setup + command dispatch |
| [turn.rs:115-238](src/turn.rs#L115-L238) | Turn orchestration (session load → LLM → tools → append → compact) |
| [turn.rs:46-97](src/turn.rs#L46-L97) | `stream_completion()` — mixes accumulation with `print!()` |
| [manifest.rs:129-140](src/manifest.rs#L129-L140) | `scan_and_load()` — discovery + merge + validate + save |
| [config.rs](src/config.rs) | User config loading |
| [model.rs](src/model.rs) | Role resolution, provider model collection |

## Proposed Solution

Three phases, each independently shippable and testable. Each phase adds one
layer of the object model and migrates the CLI to use it.

### Phase 1: `UrApp` + `UrWorkspace` (workspace operations)

Extract workspace-scoped operations into `UrWorkspace` and app-level
bootstrapping into `UrApp`. The CLI switches to constructing these objects
instead of calling free functions directly.

### Phase 2: `UrSession` (session + turn execution)

Extract session lifecycle and turn execution into `UrSession`. The core owns
the turn state machine and emits structured events. The CLI subscribes to
events for rendering.

### Phase 3: Event model + replay

Define the session event types, streaming contract, and replay model that
allow clients to restore UI state from a persisted session.

## Technical Approach

### Phase 1: `UrApp` + `UrWorkspace`

**New file:** `src/app.rs`

```rust
/// Application-level entry point. Owns the Wasmtime engine and ur_root path.
pub struct UrApp {
    engine: Engine,
    ur_root: PathBuf,
}

impl UrApp {
    /// Construct from an ur_root path. Creates the engine with caching.
    pub fn new(ur_root: PathBuf) -> Result<Self>;

    /// Open a workspace by path. Runs discovery, loads/merges manifest.
    pub fn open_workspace(&self, path: &Path) -> Result<UrWorkspace>;
}
```

**New file:** `src/workspace.rs`

```rust
/// Workspace-scoped coordinator. Owns the manifest and user config.
pub struct UrWorkspace {
    engine: Engine,       // shared (Engine is Clone/Arc-wrapped internally)
    ur_root: PathBuf,
    workspace_path: PathBuf,
    manifest: WorkspaceManifest,
    config: UserConfig,
}

impl UrWorkspace {
    // --- Extension management ---
    pub fn list_extensions(&self) -> &[ManifestEntry];
    pub fn enable_extension(&mut self, id: &str) -> Result<()>;
    pub fn disable_extension(&mut self, id: &str) -> Result<()>;
    pub fn find_extension(&self, id: &str) -> Result<&ManifestEntry>;

    // --- Role management ---
    pub fn list_roles(&self) -> &BTreeMap<String, String>;
    pub fn resolve_role(&self, role: &str) -> Result<(String, String)>;
    pub fn set_role(&mut self, role: &str, model_ref: &str) -> Result<()>;

    // --- Extension config ---
    pub fn list_extension_settings(
        &self, id: &str, pattern: Option<&str>
    ) -> Result<Vec<SettingDescriptor>>;
    pub fn get_extension_setting(&self, id: &str, key: &str) -> Result<SettingValue>;
    pub fn set_extension_setting(
        &mut self, id: &str, key: &str, value: Option<&str>
    ) -> Result<()>;

    // --- Session access (Phase 2 entry point) ---
    pub fn open_session(&self, id: &str) -> Result<UrSession>;
    pub fn create_session(&self) -> Result<UrSession>;
    pub fn list_sessions(&self) -> Result<Vec<SessionInfo>>;
}
```

**CLI migration:** `main.rs` becomes:

```rust
fn main() -> Result<()> {
    let args = Cli::parse();
    setup_tracing(args.verbose);

    let ur_root = resolve_ur_root();
    let app = UrApp::new(ur_root)?;
    let mut ws = app.open_workspace(&resolve_workspace(&args))?;

    match args.command {
        Command::Extension { action } => handle_extension(&mut ws, action),
        Command::Role { action } => handle_role(&mut ws, action),
        Command::Run => handle_run(&ws),
    }
}
```

**Files affected:**

| File | Change |
|---|---|
| `src/app.rs` | **New.** `UrApp` struct + constructor |
| `src/workspace.rs` | **New.** `UrWorkspace` struct + extension/role/config methods |
| `src/main.rs` | Slim down to CLI parsing → `UrApp` → `UrWorkspace` → dispatch |
| `src/cli.rs` | Keep parsing; move output formatting to CLI-only helpers |
| `src/manifest.rs` | No API change; called by `UrWorkspace` internals |
| `src/config.rs` | No API change; owned by `UrWorkspace` |
| `src/model.rs` | No API change; called by `UrWorkspace` |
| `src/extension_settings.rs` | Refactor `cmd_*` functions to return data, not print |

**Key decisions:**

- `UrApp` owns `Engine` creation (caching config). `Engine` is internally
  `Arc`-wrapped by Wasmtime, so cloning into `UrWorkspace` is cheap.
- `UrWorkspace` eagerly runs `scan_and_load()` on construction. This matches
  current behavior where every CLI command starts with discovery.
- Extension enable/disable on `UrWorkspace` calls `manifest::save_manifest()`
  immediately. The workspace is the source of truth; the CLI doesn't manage
  persistence.
- `extension_settings.rs` currently has `cmd_config_list/get/set` that print
  directly. Refactor these to return structured data. The CLI formats output.

**Tests:**

- Unit test `UrApp::new()` with a temp dir.
- Unit test `UrWorkspace` enable/disable/role methods using a pre-built
  manifest (no WASM needed — mock the manifest like existing tests in
  `manifest.rs`).
- Existing `make verify` continues to pass since the CLI behavior is unchanged.

### Phase 2: `UrSession`

**New file:** `src/session.rs`

```rust
/// Session-scoped coordinator. Owns session state and drives turn execution.
pub struct UrSession {
    // Internals: references to engine, manifest, config, session provider
    // instance, loaded messages, session_id, etc.
}

/// A structured event emitted during turn execution.
pub enum SessionEvent {
    /// LLM is streaming text.
    TextDelta(String),
    /// LLM emitted a complete tool call.
    ToolCall {
        id: String,
        name: String,
        arguments_json: String,
    },
    /// A tool produced a result.
    ToolResult {
        tool_call_id: String,
        tool_name: String,
        content: String,
    },
    /// The turn completed an assistant message (text only, no pending tools).
    AssistantMessage { text: String },
    /// A tool approval is required before proceeding.
    ApprovalRequired {
        id: String,
        tool_name: String,
        arguments_json: String,
    },
    /// Turn completed.
    TurnComplete,
    /// An error occurred during the turn.
    TurnError(String),
}

/// Client's response to an approval request.
pub enum ApprovalDecision {
    Approve,
    Deny,
}

impl UrSession {
    /// Session metadata.
    pub fn id(&self) -> &str;
    pub fn messages(&self) -> &[Message];

    /// Run a turn with a user message. Events are delivered via callback.
    /// The callback returns `ApprovalDecision` when it receives
    /// `ApprovalRequired`, enabling synchronous approval flow.
    pub fn run_turn(
        &mut self,
        user_message: &str,
        on_event: impl FnMut(SessionEvent) -> Option<ApprovalDecision>,
    ) -> Result<()>;
}
```

**Extraction from `turn.rs`:**

The current `turn::run()` function at [turn.rs:115-238](src/turn.rs#L115-L238)
does everything inline. Extract into `UrSession::run_turn()`:

1. `run_turn()` pushes the user message, collects tools, calls the LLM via
   streaming, and emits `SessionEvent::TextDelta` / `SessionEvent::ToolCall`
   instead of `print!()`.
2. Tool dispatch remains parallel (scoped threads), but emits
   `SessionEvent::ToolResult` per result.
3. The tool-loop (LLM → tools → LLM) continues until no tool calls remain.
4. Session append and compaction happen at the end of `run_turn()`.

**The current `stream_completion()` at [turn.rs:46-97](src/turn.rs#L46-L97)**
mixes accumulation with `print!("{delta}")`. Replace with a version that takes
an event callback:

```rust
fn stream_completion(
    llm: &mut ExtensionInstance,
    messages: &[Message],
    model_id: &str,
    settings: &[ConfigSetting],
    tools: &[ToolDescriptor],
    on_event: &mut impl FnMut(SessionEvent),
) -> Result<Completion> {
    // Same accumulation logic, but emit SessionEvent::TextDelta
    // instead of print!().
}
```

**CLI migration:**

```rust
// In main.rs handle_run():
let mut session = ws.open_session("demo")?;
session.run_turn(&user_message, |event| {
    match event {
        SessionEvent::TextDelta(delta) => {
            print!("{delta}");
            let _ = std::io::stdout().flush();
            None
        }
        SessionEvent::ToolCall { name, .. } => {
            // For now, auto-approve all tools
            None
        }
        SessionEvent::AssistantMessage { .. } => {
            println!();
            None
        }
        SessionEvent::ApprovalRequired { .. } => {
            Some(ApprovalDecision::Approve)
        }
        _ => None,
    }
});
```

**Files affected:**

| File | Change |
|---|---|
| `src/session.rs` | **New.** `UrSession`, `SessionEvent`, `ApprovalDecision`, turn execution |
| `src/workspace.rs` | Add `open_session()`, `create_session()`, `list_sessions()` |
| `src/turn.rs` | **Delete** or reduce to thin re-export. Logic moves to `session.rs` |
| `src/main.rs` | `Command::Run` uses `UrSession::run_turn()` with a printing callback |

**Key decisions:**

- **Callback, not channel.** The turn state machine is synchronous (extension
  calls are synchronous WASM). A callback is simpler than spawning an async
  event loop. The callback signature includes an `Option<ApprovalDecision>`
  return to handle approval pauses synchronously.
- **No turn object.** Per brainstorm decision 11, turns stay internal. The
  session is the unit clients hold.
- **Approval is a session event.** Per brainstorm decision 7, tool approval is
  the only domain pause initially. `ApprovalRequired` includes full tool call
  metadata so the client can render a meaningful prompt.
- **Single event stream per session.** Per brainstorm decision 5. The callback
  is called for the lifetime of `run_turn()`, not per-turn.

**Tests:**

- Unit test `SessionEvent` variants are constructible and matchable.
- Integration test: construct `UrApp` → `UrWorkspace` → `UrSession`, call
  `run_turn()` with a test callback that collects events, assert the event
  sequence matches expectations. (Requires WASM extensions; run via
  `make smoke-test` initially.)
- The existing `turn.rs` tests for `pending_session_appends` and
  `resolve_run_user_message` move to `session.rs`.

### Phase 3: Event model + replay

**Extend session persistence format.** The session provider currently stores
`Vec<Message>` — plain conversation messages. To support UI replay (brainstorm
decision 6), the persisted format must include:

- Turn boundaries and status (complete, interrupted, cancelled)
- Tool approval requests and decisions
- Domain events that affect visible session state

**New types in `session.rs`:**

```rust
/// A snapshot of session state sufficient to restore client UI.
pub struct SessionSnapshot {
    pub session_id: String,
    pub messages: Vec<Message>,
    pub events: Vec<PersistedEvent>,
}

/// An event in the persisted session timeline.
pub enum PersistedEvent {
    TurnStarted { turn_index: u32 },
    UserMessage { text: String },
    AssistantTextDelta { text: String },
    ToolCallRequested { id: String, name: String, arguments_json: String },
    ToolApprovalRequested { id: String, name: String },
    ToolApprovalDecided { id: String, decision: ApprovalDecision },
    ToolResultReceived { tool_call_id: String, content: String },
    AssistantMessageComplete { text: String },
    TurnComplete { turn_index: u32 },
    TurnInterrupted { turn_index: u32, reason: String },
}
```

**Replay contract:**

```rust
impl UrSession {
    /// Load a session and return a snapshot for UI restoration.
    pub fn snapshot(&self) -> SessionSnapshot;

    /// Replay persisted events through a callback (same signature as
    /// run_turn's callback) so the client can rebuild its UI state.
    pub fn replay(&self, on_event: impl FnMut(SessionEvent));
}
```

**Files affected:**

| File | Change |
|---|---|
| `src/session.rs` | Add `SessionSnapshot`, `PersistedEvent`, `snapshot()`, `replay()` |
| `wit/world.wit` | Potentially extend session-provider interface if persistence format changes |
| Session extension | Update to persist/load richer event format |

**Key decisions:**

- The canonical session format must be rich enough that `replay()` restores the
  final visible UI state from a single source of truth (brainstorm decision 6).
- Phase 3 does **not** require preserving every streamed token delta — only the
  final assembled messages and domain events.
- The concrete encoding (JSON-lines, protobuf, etc.) is deferred to
  implementation. The API contract is `SessionSnapshot`.
- This phase likely requires WIT changes to the session-provider interface.
  The session provider would need to store and return `PersistedEvent` records
  alongside messages. The exact WIT shape is an implementation decision.

**Tests:**

- Round-trip test: run a turn, get snapshot, replay through callback, assert
  replayed events match original events (minus streaming deltas).
- Persist a session with tool approvals, reload, verify snapshot includes
  approval events.

## Implementation Order

1. **Phase 1** — `UrApp` + `UrWorkspace`. Migrate CLI. All existing tests pass.
2. **Phase 2** — `UrSession` + `SessionEvent`. Migrate `Command::Run`. Smoke
   test passes with the new event-driven flow.
3. **Phase 3** — Replay model. Requires session provider WIT changes. Can be
   deferred until a second client (TUI) needs replay.

Each phase ends with `make verify && make smoke-test` green.

## Open Questions

1. **Session ID generation.** Currently hardcoded to `"demo"`. Phase 2 needs
   `create_session()` to generate IDs. UUID? Timestamp-based? Delegate to the
   session provider?
2. **Multi-turn loop.** `run_turn()` handles one user message → LLM → tools →
   LLM cycle. A REPL loop that calls `run_turn()` repeatedly is the CLI's job.
   But should `UrSession` expose a `run_loop()` helper that drives the REPL?
   Probably not — keep it simple, let clients own the loop.
3. **Async future.** The current design is synchronous. If a future GUI client
   needs async, `run_turn()` could become `async` or return a handle. Not
   needed now, but the callback-based design doesn't preclude it.

## Rust Development Note

This plan involves writing new Rust files. Invoke `/krust` before
implementation to ensure compliance with Rust development guidelines.
