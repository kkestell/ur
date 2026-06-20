# Models

## What is a `Model`?
A provider-bound handle pairing a `Provider` with a model id and generation settings.

## Construction and catalog lookup
`Model::new(provider, id)` resolves `ModelSpec` and any `ModelNotice` exactly once.

## Catalog accessors
`id()`, `context_window()`, and `max_output()` and their behavior for unknown ids.

## Settings
The consuming-builder setters and what each controls.

### `max_tokens`
Output token cap and validation against catalogued `max_output`.

### `temperature` / `top_p`
Sampling knobs and provider-specific range rules.

### `stop`
Stop sequences and provider-specific entry limits.

### `response_format`
Pointer to the structured-outputs doc for the response-format enum.

### `thinking`
The `Thinking` enum (`Default`/`Enabled`/`Disabled`) and per-provider honoring.

### `reasoning_effort`
The `ReasoningEffort` enum (`Low`–`Max`) and provider aliasing behavior.

### `tool_choice`
The `ToolChoice` enum, which sets the tool selection policy for a turn. `Auto` (the default) lets the model decide whether to call a tool, `None` forbids tool calls while leaving registered tools in scope, `Required` forces the model to call some tool, and `Tool(name)` forces the named tool. It takes effect only when the agent has registered tools.

## Validation
Which settings are checked locally and surface as `Error::Config` before any request.

## Model notices
The `ModelNotice::Deprecated` path and the single `tracing` warning emitted at construction.

## Clone semantics
`Model` clones cheaply via an `Arc` provider, so settings can diverge per clone.
