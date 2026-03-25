# Ur

Ur is an agentic coding assistant with a lean Rust core and a Lua extension system.

The core handles the agent loop, LLM providers, session storage, and compaction. Extensions add tools and hooks via sandboxed Lua scripts.

Extensions run in isolated Luau VMs with deny-by-default permissions: no filesystem or network access unless explicitly declared in `extension.toml`. Each VM is memory-limited (64 MB) and instruction-budgeted.