# Ur

Ur is an agentic coding assistant with a tiny Rust core and a rich extension system. 

The core handles the agent loop and extension hosting. Everything else — LLM providers, tools, session storage, context — is an extension.

Extensions are WebAssembly components running in wasmtime. They're sandboxed with deny-by-default permissions: no filesystem or network access unless explicitly declared. 

Extensions can be written in any language that compiles to the WASM Component Model, using typed bindings generated from a WIT API.