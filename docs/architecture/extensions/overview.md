# Extension System Overview

ur uses WebAssembly (WASM) components for its extension system, providing a secure, portable, and composable plugin architecture. This document provides a high-level overview of the technologies involved and how they work together.

## Goals

The extension system is designed around three core principles:

- **Sandboxing** — Extensions run in isolated WASM components with no direct access to the host system
- **Portability** — Extensions can be written in any language that compiles to WASM components
- **Composability** — Extensions declare capabilities through well-defined interfaces, enabling the host to route requests appropriately

## Technology Stack

### Wasmtime

[Wasmtime] is the WebAssembly runtime that executes extension code. It implements the Component Model, allowing type-safe communication between the host (ur) and guest (extension) code. Wasmtime provides:

- JIT compilation of WASM components
- Resource management for handles passed across the host-guest boundary
- Integration with WASI for system capabilities

[Wasmtime]: https://wasmtime.dev/

### WIT (WebAssembly Interface Types)

[WIT] is an interface definition language for describing the contracts between hosts and guests. In ur, WIT files define:

- **Types** — Shared data structures like `Message`, `Completion`, `ToolCall`, etc.
- **Interfaces** — Groups of related functions (e.g., `host`, `llm-provider`)
- **Worlds** — Complete contracts specifying what a component imports and exports

The WIT definitions live in `wit/world.wit` and serve as the single source of truth for the extension API.

[WIT]: https://component-model.bytecodealliance.org/design/wit.html

### wit-bindgen

[wit-bindgen] generates Rust code from WIT definitions for both sides of the boundary:

- **Host side** — Generates traits and glue code that ur implements to provide platform services
- **Guest side** — Generates Rust types and function signatures that extensions implement

This eliminates manual serialization and ensures type safety across the WASM boundary.

[wit-bindgen]: https://github.com/bytecodealliance/wit-bindgen

### WASI Preview 2

[WASI Preview 2] (also known as "p2" or the Component Model version) provides standardized interfaces for system capabilities. ur uses WASI p2 to give extensions controlled access to:

- Standard I/O (inherited from the host process)
- Clocks and random number generation
- File system access (when configured)

WASI Preview 2 is a significant evolution from Preview 1, built around the Component Model and WIT-based interface definitions.

[WASI Preview 2]: https://github.com/WebAssembly/WASI/tree/main/wasip2

### wasi-http

For extensions that need network access, ur integrates [wasi-http] — the WASI HTTP interface. This allows extensions to make outbound HTTP requests using a standardized API. Extensions targeting `llm-provider` slots that call external APIs (like Google Gemini or Anthropic) use wasi-http for network communication.

Importantly, wasi-http provides only the client side — extensions can make outgoing requests but cannot listen for incoming connections.

[wasi-http]: https://github.com/WebAssembly/wasi-http

## How It All Fits Together

The extension lifecycle follows this flow:

```
┌─────────────────────────────────────────────────────────────────┐
│                        Development                               │
│                                                                  │
│  wit/world.wit ─────► wit-bindgen ─────► Rust bindings          │
│       (WIT)              (generate)          (host + guest)      │
│                                                                  │
│  Extension code implements guest interfaces                      │
│  Host code implements host interfaces                            │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                         Build                                    │
│                                                                  │
│  Extension Rust ─────► cargo build ─────► .wasm component       │
│  (guest side)            (target wasm32-wasip2)                  │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                        Runtime                                   │
│                                                                  │
│  ur (host) ─────► Wasmtime ─────► Extension instance            │
│     │                │                    │                      │
│     │                │                    │                      │
│     │           WASI p2             Guest exports:               │
│     │          wasi-http            - init()                     │
│     │                                - complete()                 │
│     │                                - etc.                       │
│     │                                                             │
│     └──── Implements host interface ◄──── Extension calls        │
│           - log()                       host.complete()          │
│           - complete()                  host.log()               │
│           - etc.                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### The Component Model

The [Component Model] is the foundation that ties everything together. Unlike core WASM modules, components:

- Communicate via typed interfaces defined in WIT
- Share rich data structures (strings, records, variants, lists) without manual memory layout
- Support resources — handles to host-managed objects that can be passed across the boundary

This enables ur to pass complex types like `Message` and `Completion` directly to extensions without serialization overhead.

[Component Model]: https://component-model.bytecodealliance.org/

## Extension Slots

ur organizes extensions into *slots* based on the capabilities they provide. A slot is a named extension point with defined cardinality rules:

| Slot | Cardinality | Required | Purpose |
|------|-------------|----------|---------|
| `llm-provider` | At least one | Yes | Provides LLM completions |
| `session-provider` | Exactly one | Yes | Persists conversation history |
| `compaction-provider` | Exactly one | Yes | Summarizes/compacts message history |
| (none) | Unlimited | No | General-purpose extensions |

**Cardinality** determines how many extensions can fill a slot simultaneously:

- **Exactly one** — A switch; enabling a new extension disables the previous one
- **At least one** — Multiple providers can coexist; the host selects based on context

### Worlds

Each slot corresponds to a WIT *world* — a complete specification of what the component imports and exports:

- **`llm-extension`** — LLM providers without HTTP
- **`llm-extension-http`** — LLM providers with HTTP (imports `wasi:http/outgoing-handler`)
- **`session-extension`** — Session persistence providers
- **`compaction-extension`** — Compaction providers
- **`general-extension`** — Extensions with no slot

## Host-Guest Communication

Communication flows bidirectionally across the component boundary:

### Imports (Extension → Host)

Extensions import the `host` interface to access platform capabilities:

```wit
interface host {
    log: func(msg: string);
    complete: func(messages: list<message>, role: option<string>) -> result<completion, string>;
    load-session: func(id: string) -> result<list<message>, string>;
    // ...
}
```

This allows extensions to:
- Log messages through the host
- Request completions from the active LLM provider
- Access session storage

### Exports (Host → Extension)

The host calls functions that extensions export. All extensions must export the base `extension` interface:

```wit
interface extension {
    init: func(config: list<config-entry>) -> result<_, string>;
    call-tool: func(name: string, args-json: string) -> result<string, string>;
    id: func() -> string;
    name: func() -> string;
}
```

Slot-specific interfaces add additional exports. For example, `llm-provider` extensions also export:

```wit
interface llm-provider {
    provider-id: func() -> string;
    list-models: func() -> list<model-descriptor>;
    complete: func(messages: list<message>, model: string, settings: list<config-setting>) 
        -> result<completion, string>;
}
```

## Networked Extensions

Extensions that need to make HTTP requests use the `wasi:http/outgoing-handler` import. The `llm-extension-http` world includes this capability:

```wit
world llm-extension-http {
    import host;
    import wasi:http/outgoing-handler@0.2.6;
    export extension;
    export llm-provider;
    export llm-streaming-provider;
}
```

At runtime, ur configures the Wasmtime linker with `wasi-http` support, allowing extensions to make outbound requests. The host controls network access — extensions without the HTTP import cannot make network calls.

## Summary

ur's extension system combines several WebAssembly technologies to create a secure plugin architecture:

1. **WIT** defines typed interfaces between host and guest
2. **wit-bindgen** generates Rust bindings from WIT definitions
3. **Wasmtime** executes compiled WASM components
4. **WASI Preview 2** provides controlled system access
5. **wasi-http** enables network requests for extensions that need them

Together, these technologies enable ur to load untrusted extensions safely while providing rich capabilities through well-defined interfaces.