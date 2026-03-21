# Ur

Ur is an agentic coding assistant with a tiny core and a rich extension system.

The core is written in Rust for safety and performance. It handles the agent loop, session storage, and extension hosting — nothing else. There is exactly one built-in tool (`reload`). Everything else — file I/O, shell access, LLM providers, context management — is an extension.

extensions are WebAssembly components running inside [wasmtime](https://wasmtime.dev/). They execute in a sandbox with a deny-by-default permission model: no ambient filesystem or network access. extensions must explicitly declare every path they read, every path they write, and every network host they contact. Undeclared access is denied at runtime. This makes extension permissions fully auditable from the manifest alone, without running the extension.

Any language that compiles to the WASM Component Model can be used to write extensions. The extension API is defined in [WIT](https://component-model.bytecodealliance.org/design/wit.html), so extension authors work with generated, typed bindings rather than raw WASM imports.

