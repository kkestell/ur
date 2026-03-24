# Brainstorm: Front-End Separation and Core Application API

**Date:** 2026-03-22
**Status:** Complete

## What We're Building

Separate ur's presentation concerns from its core behavior so the same host logic can power:

- the current noninteractive CLI
- a future interactive CLI
- a proper TUI
- a GUI

The core idea is to make ur expose a front-end-neutral application API. Front ends should parse input, invoke use cases, and render output. ur itself should own orchestration, extension hosting, session behavior, model resolution, and persistence, but it should stop deciding how humans see those results.

This is especially important for `run`, where streaming text, tool activity, trace messages, and extension logs are currently written directly to the terminal.

## Why This Approach

ur's architecture already wants a tiny core. The README says the core should handle the agent loop, session storage, and extension hosting "nothing else." Right now, that boundary is blurred:

- `src/main.rs` mixes command dispatch, persistence, and `println!`
- `src/model.rs` contains both model-resolution logic and CLI command handlers
- `src/turn.rs` performs orchestration and renders progress directly to stdout
- `src/extension_host.rs` routes host logs to `println!` and inherits stdio

That makes the terminal the de facto API. A TUI or GUI would either duplicate logic or force more conditionals into the core. A front-end-neutral application API fixes the seam once and lets every UI become an adapter instead of a rewrite.

## Approaches Considered

### Recommended: In-process application services with typed responses and typed events

Create a core API layer around use cases such as extension management, model management, and turn execution. Simple operations return typed result objects. Long-running operations like `run` also emit typed events for streaming text, tool calls, warnings, logs, and lifecycle milestones.

**Pros**
- Best balance of cleanliness and simplicity
- Keeps core logic reusable without introducing transport complexity
- Gives TUI and interactive CLI the event stream they need
- Lets the existing CLI become a thin adapter

**Cons**
- The first GUI would likely need to be Rust-based or embedded
- Requires a deliberate output model instead of ad hoc printing

**Best suited when**
- We want clean separation now without committing to IPC, HTTP, or daemon architecture

**Design quality lens**
- **SRP:** Strong. Core handles use cases; front ends handle rendering.
- **OCP / DIP:** Strong. New front ends depend on stable request/response and event abstractions.
- **YAGNI / KISS:** Strong. Solves today's problem without inventing a network protocol.
- **Value Objects:** Good fit for `RoleName`, `ModelRef`, `SessionId`, `ExtensionId`, and event kinds.
- **Complexity:** Low-to-medium. The main new concept is an event model for interactive work.

**Object stereotypes**
- Application API: `Coordinator`
- Use-case services: `Service Provider`
- Result and event types: `Information Holder`
- CLI/TUI/GUI adapters: `Interfacer`

### Alternative: Event-first runtime for all operations

Push every user-visible action through a single event-driven runtime surface, even list/get/set style commands. Front ends consume an event stream and decide what to display or persist.

**Pros**
- Very consistent abstraction
- Excellent fit for streaming, tracing, and interactive UX
- Makes rich observability natural

**Cons**
- Heavier mental model for simple commands
- Risks over-designing before the product needs it

**Best suited when**
- We expect most future features to be conversational, interactive, or multi-step

**Design quality lens**
- **SRP:** Good, but some simple commands may feel artificially eventful.
- **OCP / DIP:** Very strong.
- **YAGNI / KISS:** Weaker than the recommended approach.
- **Value Objects:** Strong fit for event payloads and state transitions.
- **Complexity:** Medium.

**Object stereotypes**
- Runtime/event bus: `Coordinator`
- Event payloads: `Information Holder`
- Front-end renderers: `Interfacer`

### Alternative: Cross-process API (JSON-RPC/IPC/daemon)

Treat ur core as a service and make every UI a client over an explicit protocol.

**Pros**
- Strongest decoupling
- Opens the door to non-Rust GUIs or external integrations
- Creates a stable external contract

**Cons**
- Adds transport, lifecycle, versioning, and error-shaping concerns early
- Much more machinery than the current product needs
- Risks building a platform before finishing the core

**Best suited when**
- A separate desktop app, editor integration, or multi-language client is an immediate goal

**Design quality lens**
- **SRP:** Mixed. Transport concerns become first-class immediately.
- **OCP / DIP:** Strong externally, but with more layers to maintain.
- **YAGNI / KISS:** Weak for current scope.
- **Value Objects:** Requires protocol DTOs in addition to domain concepts.
- **Complexity:** High.

**Object stereotypes**
- RPC service: `Controller`
- Protocol layer: `Structurer`
- Clients/renderers: `Interfacer`

## Key Decisions

1. **Adopt a front-end-neutral application API inside ur first.**
2. **Keep the first boundary in-process as an internal Rust library, not cross-process.** A transport protocol can wrap the same use cases later if needed.
3. **Model long-running operations as typed events, not terminal output.** `run` is the primary driver here.
4. **Move all formatting responsibility to adapters.** Tables, prompts, bullet traces, and streamed text presentation belong to CLI/TUI/GUI layers.
5. **Treat all human-visible output as presentation, including extension logs and tracer messages.** The terminal should no longer be the implicit sink.
6. **Use the same core use cases for every front end.** No separate CLI-only business logic.

## Open Questions

- **Future transport:** When a GUI arrives, should it embed the Rust library directly or should we add a transport wrapper then?
- **Event scope:** Should only interactive flows emit events, or should even simple commands expose lifecycle/warning events for consistency?
- **Extension stdio:** Do we continue inheriting guest stdio, or should logs/stdout/stderr be captured and routed through the same front-end-neutral output channel?
- **Hardcoded `run` flow:** Should the first API only separate presentation, or should it also generalize `run` beyond the current demo turn while we are touching the boundary?
