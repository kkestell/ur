# Tools

## What is a tool?
The `Tool` concept: a named, schema-described callable the model can invoke.

## Stateless tools with `#[ur::tool]`
Annotating a free `async`/sync fn to produce a registrable tool.

### The `description` attribute
Why every tool needs a description and how it reaches the model.

### Parameters and JSON Schema
How fn parameters are turned into a JSON Schema via `schemars` `JsonSchema`.

### Return types and errors
What return shapes are accepted (serializable values, `Result<T, E>`) and how they serialize.

## Stateful tools with `#[ur::tools]`
Turning an inherent impl block's `&self` methods into a `ToolSet` sharing cloned state.

### The owning state type
The `Clone + Send + Sync + 'static` requirement and why `Arc<_>` fields keep cloning cheap.

### Interior mutability
Using `Arc<AtomicX>` / `Arc<Mutex<_>` rather than `&mut self`.

## Registering tools
`.tool()`, `.tools()`, `.tool_set()`, registration order, and duplicate/invalid-name detection.

## `ToolArguments`
The raw-JSON argument type and its `parse::<T>()` / `as_str()` helpers.

## `ToolSchema`
The `name`/`description`/`parameters`/`strict` record and its builder methods.

## Strict mode (tools)
What strict mode means for tool schemas and where the shared rewriter is specified.

## Implementing `Tool` by hand
When and how to implement the trait directly instead of using a macro.

## The `ToolSet` trait
What the macro-generated `ToolSet` exposes and `into_tools()`.
