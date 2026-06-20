# Agents

## The mental model
The layered types — Provider → Model → Agent → Session → EventStream — and who owns what.

## `Agent`
What an `Agent` is: a reusable definition of system prompt + model + tools.

### Construction
`Agent::new(system_prompt, model)` and the consuming-builder registration methods.

### Registering tools
`.tool()`, `.tools()`, and `.tool_set()` and their ordering semantics.

### Clone and reuse
`Agent` is cheaply cloneable (Arc internals) so one definition spawns many sessions.

## `Session`
What a `Session` is: one conversation carrying independent mutable history.

### Starting a session
`agent.session()` and how siblings share the agent but not history.

### Sending a turn
`session.send(...)` returns an `EventStream` borrowed by the session.

### Conversation history
`history()`, full-history replay on every turn, and `reset()` back to the system prompt.

## The turn lifecycle
How a single `send` flows: provider stream → tool rounds → terminal `Done`, and when history commits.

## Rollback semantics
When and why a pending turn is discarded (provider error, early stream drop, malformed tool calls) and what is preserved.

## Validation before the network
Which misconfigurations surface as `Error::Config` before any request is sent.
