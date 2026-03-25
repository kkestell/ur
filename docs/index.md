# Ur

Ur is an agentic coding assistant with a lean Rust core and a Lua extension system.

The core handles the agent loop, LLM providers, session storage, and compaction. Extensions add tools and lifecycle hooks via sandboxed Lua scripts.

## Documentation

### Using Ur

- [Extensions](extensions.md) — Installing, enabling, and managing extensions

### Development

- [Developing Extensions](development/extensions/index.md) — Creating your own extensions
  - [Lifecycle Hooks](development/extensions/hooks.md) — Hook reference with context fields and examples
