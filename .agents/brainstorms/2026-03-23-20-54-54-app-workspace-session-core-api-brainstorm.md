# App Workspace Session Core API

**Date:** 2026-03-23
**Status:** Brainstorm complete, ready for planning

## What We're Building

A headless core API for Ur that cleanly separates client surfaces from the
application/runtime internals. The immediate goal is to let the current CLI sit
on top of this API without owning orchestration logic, while also making room
for future TUI and GUI clients.

The core shape is:
- `UrApp` for app-wide concerns and workspace access
- `UrWorkspace` for workspace-scoped operations
- `UrSession` for the runtime and persisted conversation/session model

`UrWorkspace` owns non-conversation workspace behavior such as extension
discovery, enable/disable, extension config, role/model config, and
listing/opening/creating sessions. `UrSession` has a 1:1 relationship with the
session concept exposed by the enabled session-provider extension, and is the
place where clients run turns, subscribe to streaming updates, inspect current
state, replay session state, and answer domain-level decisions such as tool
approvals.

The API must be rich and typed. Clients need structured access to turn events,
approval requests, tool names, arguments, session state, and replayable history.
Opaque log strings are explicitly out of scope as the primary interface.

## Why This Approach

Today, the codebase already hints at a useful split, but the binary still owns
too much orchestration. [src/main.rs](/Users/kyle/src/ur/src/main.rs) parses the
CLI and directly coordinates manifest loading, provider discovery, config
updates, and turn execution. [src/turn.rs](/Users/kyle/src/ur/src/turn.rs) is
already close to an application/runtime coordinator, while
[src/cli.rs](/Users/kyle/src/ur/src/cli.rs) mixes parsing with presentation.

The chosen model avoids two problems at once. First, it prevents the CLI from
remaining the de facto application layer. Second, it avoids forcing clients to
learn two unrelated top-level APIs for admin work versus turn execution.

We considered a facade-plus-runtime split, but rejected it because it would make
clients learn two separate concepts up front. We also do not want to jump
straight to a protocol-first or daemon-first architecture. The chosen design is
in-process first, but should use typed requests, results, events, snapshots, and
IDs so that an IPC or RPC layer could be added later without redesigning the
domain model.

This is the simplest design that supports streaming, replay, approvals, and rich
UI rendering without baking terminal behavior into the core.

## Key Decisions

### 1. Headless Core

The shared core must contain no terminal or UI I/O. It exposes structured
commands, domain objects, events, snapshots, and decisions only. Rendering
prompts, token output, approval modals, and local UX polish belong entirely to
clients.

### 2. Top-Level Object Model

Use a three-level model:
- `UrApp`: app-wide controller and workspace manager
- `UrWorkspace`: workspace-scoped controller/coordinator
- `UrSession`: durable session entity plus live runtime interface

This gives clients one coherent root model instead of separate "admin" and
"runtime" APIs.

### 3. Session Semantics

`UrSession` is both persisted and live. It represents the same underlying
session concept used by the active session provider, not a separate client-side
abstraction. A client can inspect it, resume it, stream updates from it, replay
its state, and run turns against it.

### 4. Interactive Contract

Turn execution is core-owned. The core should run a state machine and expose:
- typed streaming events for live UX
- a structured snapshot for inspection and reconnect/replay
- typed decision requests for domain pauses

This is especially important for tool approvals. Clients must receive the exact
approval target and all needed metadata, including tool name, arguments, call
identity, and related context.

### 5. Session Event Model

Each `UrSession` should expose one event stream over the lifetime of the
session, rather than creating a separate top-level client concept per turn. The
session remains the main object clients hold onto, and turn activity appears as
structured events within that session stream.

This keeps the client model simple while still leaving room for turn IDs or
turn records internally if the implementation needs them later.

### 6. Replay Model

Replay should cover everything necessary to restore the UI to the state it was
in when the session was last ended. That implies a richer replay model than
persisted conversation messages alone. The system should preserve enough
structured session state and user-visible execution history for a client to
reconstruct the last meaningful session view.

The contract should target UI restoration, not just transcript recovery. The
current preferred direction is to make the canonical session persistence format
rich enough to reconstruct the final visible client state directly, rather than
splitting persistence across a transcript plus a separate replay artifact. This
means the session log likely needs to capture more than plain chat messages,
including domain events and state transitions that affect the visible session
state. It does not require preserving every streamed token delta if the goal is
to restore the final UI state rather than replay the exact live timeline.

### 7. Approval Policy Surface

For the initial design, tool approval is the only domain pause that must be in
the core contract. The core should be designed so additional domain-level
decision points can be added later without reshaping the entire API.

### 8. Responsibility Boundaries

Major components should stay narrow:
- `UrApp`: Interfacer/Controller for application entry and workspace access
- `UrWorkspace`: Coordinator for workspace capabilities
- `UrSession`: Coordinator over session lifecycle and turn execution
- manifests/config/session records/events/approval payloads: Information Holders
- lower-level discovery/config/provider/runtime services: Service Providers

The point is not to create one giant object. The point is to give clients a
coherent model while keeping implementation responsibilities delegated to
specialized collaborators.

### 9. `UrApp` Scope

In the initial design, `UrApp` is mainly a root for locating, opening, and
managing `UrWorkspace` instances. No additional app-wide operations need to be
first-class yet.

### 10. Transport-Friendly, Not Transport-First

The first implementation should be an in-process Rust API. But the contracts
should already be structured enough that a future IPC layer could forward the
same kinds of commands, events, snapshots, and decisions with minimal churn.

### 11. Turn Visibility

Turns should remain internal to `UrSession` for now. The public API should stay
centered on the session itself and its lifetime event stream, rather than
introducing inspectable turn objects as first-class client concepts.

This keeps the surface area smaller and matches the goal of giving clients one
coherent object to hold onto. If future clients need deeper turn inspection, it
can be added later without making it part of the initial mental model.

## Open Questions

### Canonical Session Format Shape

The remaining question is not whether the canonical session log must be rich
enough to restore final visible UI state from a single source of truth; that is
decided. The remaining design question is how exactly to encode that timeline
in the persisted session format.

The current minimum expected domain data is:
- turn boundaries and turn status
- user, assistant, and tool messages
- tool approval requests and approval decisions
- tool calls and tool results
- in-progress, interrupted, cancelled, or otherwise incomplete status markers

This list is the working baseline for planning. The implementation still needs
to decide the concrete representation and how much normalization or event
granularity is appropriate.
