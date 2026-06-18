# `ur` — API specification

`ur` is an async Rust facade for driving tool-using LLM agents. It owns the full agent loop — streaming, reasoning, tool dispatch, multi-turn history — over a pluggable provider backend. Providers ship as separate crates, enabled by feature.

This document is self-contained for the provider-agnostic surface: it specifies every public item of `ur`, `ur-core`, and `ur-macros`, the `#[ur::tool]` macro contract, the agent loop semantics, and the `Provider` seam that backends implement. It deliberately says nothing about any concrete provider's wire format, models, or runtime behavior; that belongs in each provider crate's own documentation.

- Edition: 2024 (Rust 1.88+). MSRV: 1.88.
- Async runtime: agnostic at the type level. A concrete provider may impose a runtime requirement (for example, an HTTP provider built on `reqwest` requires a Tokio reactor at run time).

---

## 1. Workspace layout

`ur` is a Cargo workspace. The user-facing `ur` crate is a thin facade that re-exports `ur-core` and conditionally re-exports each enabled provider crate. The core types, the macro, and every provider live in their own crates.

| Crate       | Role                                                                                                     | Depends on                                       |
| ----------- | -------------------------------------------------------------------------------------------------------- | ------------------------------------------------ |
| `ur`        | facade: re-exports `ur-core`; gates providers behind features                                            | `ur-core`, `ur-macros`, optional provider crates |
| `ur-core`   | all provider-agnostic types: `Agent`, `Model`, `Session`, `Event`, `Tool`, the `Provider` trait, `Error` | `ur-macros`                                      |
| `ur-macros` | the `#[tool]` proc-macro                                                                                 | —                                                |

Each provider is its own crate (conventionally `ur-<name>`) that implements the [`Provider`](#8-provider-seam) trait, depends only on `ur-core`, and is gated behind a matching feature on the facade. Nothing in `ur-core` knows about a concrete provider, and provider crates do not depend on one another.

`ur-core` module layout:

```
ur_core
├── (root)            Agent, Model, Session, UserMessage, EventStream, Error, Result, BoxFuture, BoxStream, Stream, JsonError, JsonSchema, JsonValue
├── model             Thinking, ReasoningEffort, ResponseFormat
├── tool              Tool trait, ToolSchema, ToolArguments
├── event             Event, FinishReason, Usage, ToolOutput
└── provider          Provider trait, Request, RawEvent, Message, MessageRole, ToolCall, ModelSpec, ModelNotice, Settings
```

Facade (`ur`) re-exports (the names an app author uses directly):

```rust
pub use ur_core::{Agent, Model, Session, UserMessage, EventStream, Error, Result};
pub use ur_core::{BoxFuture, BoxStream, JsonError, JsonSchema, JsonValue, Stream};
pub use ur_core::event::{Event, FinishReason, Usage};
pub use ur_core::model::{Thinking, ReasoningEffort, ResponseFormat};
pub use ur_core::tool::{Tool, ToolArguments, ToolSchema};
pub use ur_core::event::ToolOutput;
pub use ur_core::provider::{Provider, Message, MessageRole, ToolCall, Request, RawEvent, ModelSpec, ModelNotice, Settings};
pub use ur_macros::tool;            // the #[ur::tool] attribute macro

// Each enabled provider feature re-exports its crate under `ur`, e.g.:
//   #[cfg(feature = "<provider>")]
//   pub use ur_<provider> as <provider>;
```

With no provider feature at all, `ur` still compiles and exposes the full agent/tool surface; there is simply no concrete `Provider` to construct.

### Cargo metadata (C-METADATA)

```toml
# ur/Cargo.toml
[package]
name = "ur"
edition = "2024"
rust-version = "1.88"
description = "Async tool-using LLM agents over a pluggable provider backend"
license = "MIT OR Apache-2.0"
repository = "..."         # fill in
documentation = "https://docs.rs/ur"
authors = ["..."]          # fill in
keywords = ["llm", "agent", "ai", "tools", "async"]
categories = ["api-bindings", "asynchronous"]
```

### Feature flags (on the `ur` facade)

| Feature | Default | Effect                                                                                                                                                                                                                                                            |
| ------- | ------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `serde` | on      | `Serialize`/`Deserialize` on the conversation and provider data types: `Event`, `ToolOutput`, `Usage`, `FinishReason`, `Message`, `MessageRole`, `ToolCall`, `ToolArguments`, `ToolSchema`, `Request`, `RawEvent`, `ModelSpec`, `ModelNotice`, `UserMessage`, `Settings`, `Thinking`, `ReasoningEffort`, `ResponseFormat` |

Each provider is an optional-dependency feature on the facade in the shape `ur-<name> = { optional = true }`, `<name> = ["dep:ur-<name>"]`. Enabling the feature pulls in the provider crate and re-exports it as `ur::<name>`. A provider feature may be included in `default` for turnkey use; whether it is, and the model ids it offers, are documented by that provider crate.

In this workspace, `openai` is default-on for turnkey `cargo add ur` usage and `deepseek` is optional-only. Provider-free builds still work with `default-features = false`.

`ur-macros` and `serde` are pulled in by `ur-core` unconditionally for tool support (`schemars` for JSON-Schema generation, `serde` for argument de/serialization); tools cannot work without them. `ur-core` also depends on `tracing` to emit model-construction notices such as provider deprecation warnings. The facade's `serde` feature only toggles the _public_ `Serialize`/`Deserialize` impls listed in the feature table above. The intent is that callers can serialize conversations, provider test fixtures, and settings snapshots when the feature is enabled.

Public aliases keep common dependency names under the `ur` namespace:

```rust
pub type BoxFuture<'a, T> = futures_core::future::BoxFuture<'a, T>;
pub type BoxStream<'a, T> = futures_core::stream::BoxStream<'a, T>;
pub use futures_core::Stream;
pub use schemars::JsonSchema;
pub use serde_json::{Error as JsonError, Value as JsonValue};
```

Stability note: core public signatures use these `ur::` names rather than asking callers to spell `futures-core`, `schemars`, or `serde_json` paths directly. `futures-core` is a pre-1.0 public dependency, but the 0.3 line is ecosystem-stable and is intentionally exposed through `ur::BoxFuture`, `ur::BoxStream`, and `ur::Stream` before any 1.0 commitment.

---

## 2. Errors

```rust
pub type Result<T, E = Error> = std::result::Result<T, E>;

#[non_exhaustive]
#[derive(Debug)]
pub enum Error {
    /// Authentication failed: missing or invalid credentials. Parameterless: the
    /// condition is determined by the provider and the (generic) message is not
    /// retained.
    Auth,
    /// The account has insufficient balance or quota. Parameterless, as for `Auth`.
    InsufficientFunds,
    /// The provider rejected the request as malformed.
    BadRequest { message: String },
    /// The request was well-formed but a parameter value was rejected.
    InvalidParams { message: String },
    /// A rate or concurrency limit was reached. Retried automatically up to the
    /// configured limit; surfaced only after retries are exhausted.
    RateLimited { retry_after: Option<std::time::Duration> },
    /// A retryable server-side failure, or an otherwise-unmapped provider status.
    Server { status: u16, message: String },
    /// The connection or transport layer failed.
    Transport(Box<dyn std::error::Error + Send + Sync>),
    /// A response or stream chunk could not be decoded.
    Decode { context: String, source: Box<dyn std::error::Error + Send + Sync> },
    /// A client or model setting was invalid — detected locally, before any
    /// request is sent: no credentials available, malformed configuration, or an
    /// out-of-range generation setting (see §3, §4).
    Config { message: String },
}
```

- Implements `std::error::Error` and `Display`; is `Send + Sync + 'static` (C-GOOD-ERR). `Transport` and `Decode` expose their source via `Error::source`.
- These variants are the shared error vocabulary every provider maps onto. A provider is responsible for translating its transport/HTTP failures into them; the exact mapping is documented by that provider crate.

---

## 3. Model — `Model<P>`

Binds a provider to a model id plus the per-turn generation settings. Generic over the provider so the same `Agent`/`Session` machinery works for any backend.

```rust
pub struct Model<P: Provider> { /* private */ }

impl<P: Provider> Model<P> {
    /// `model_id` is the provider's identifier for a model. Context window and max
    /// output are looked up from the provider's catalog (see `Provider::model_spec`);
    /// they are NOT caller-supplied. Provider notices, such as deprecation
    /// warnings, are resolved here as well.
    pub fn new(provider: P, model_id: impl Into<String>) -> Self;

    // ---- read-only model facts (from the catalog; None if id is unknown) ----
    pub fn id(&self) -> &str;
    pub fn context_window(&self) -> Option<u32>;
    pub fn max_output(&self) -> Option<u32>;

    // ---- generation settings (chainable, consuming) ----
    pub fn thinking(self, mode: Thinking) -> Self;
    pub fn reasoning_effort(self, effort: ReasoningEffort) -> Self;
    pub fn max_tokens(self, n: u32) -> Self;
    pub fn temperature(self, t: f32) -> Self;
    pub fn top_p(self, p: f32) -> Self;
    pub fn stop(self, seqs: impl IntoIterator<Item = String>) -> Self;
    pub fn response_format(self, fmt: ResponseFormat) -> Self;
}
```

```rust
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Thinking {
    #[default]
    Default,
    Enabled,
    Disabled,
}

/// A hint for how much reasoning effort the model should spend. Providers map
/// these levels onto their own scale; an unsupported level is the provider's to
/// interpret.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ReasoningEffort { Low, Medium, High, ExtraHigh, Max }

#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ResponseFormat {
    #[default]
    Text,
    JsonObject,
}
```

Notes:

- **Generation settings are recorded as set and carried on the `Request`.** How a provider applies each one — including whether some are conditioned on `Thinking` mode, and the accepted value ranges — is provider-defined.
- **Catalog facts and model notices are resolved at construction.** `Model::new` calls `Provider::model_spec` and `Provider::model_notice` exactly once for the supplied id. It caches the `ModelSpec` result for all later accessors, request construction, and `max_tokens` validation. If `model_notice` returns `Some(ModelNotice::Deprecated { message })`, `Model::new` emits one `tracing` warning for that model construction. Later `context_window()`, `max_output()`, settings builders, and sends do not re-query the provider and do not re-emit the warning.
- **`max_tokens` is validated locally against the catalog.** When set, it is checked as at least `1` and, for catalogued models, at most `max_output` (from the provider's catalog via `Provider::model_spec`); unknown model ids have no local upper cap. Other generation settings are validated by the provider: an invalid value surfaces as `Error::Config` (when the provider detects it locally, before any request) or `Error::InvalidParams` (when the service rejects it).
- The generation-setting builders are intentionally infallible and chainable. Values such as `temperature(999.0)` or an unsupported `top_p` are recorded on the model and fail later from `Session::send`, when the provider validates the complete request.
- Unknown model ids are permitted. `context_window()` and `max_output()` return `None` for them, and the provider decides how to treat them.

---

## 4. Tools

### The `#[ur::tool]` macro

Applied to an `async fn`, it generates a value implementing [`Tool`](#the-tool-trait) **bound to the same identifier as the function**, so `agent.tool(add)` works.

```rust
#[ur::tool(description = "Add two integers.")]
async fn add(a: i64, b: i64) -> i64 { a + b }
```

Contract:

- **Return type** is either `T` or `Result<T, E>` where `T: Serialize` and `E: Display`. A bare `T` is treated as infallible. On success the macro serializes `T` to a JSON string for the tool result; on `Err(e)` it produces `Err(e.to_string())`. If the model supplies malformed arguments that fail to deserialize into the generated parameter struct, the macro also returns `Err(e.to_string())`. Tool errors travel back to the model as that message; there is no separate tool-error channel (see §6).
- **Parameters** must each be `DeserializeOwned + ur::JsonSchema`. The macro synthesizes a private params struct from the argument list, derives `Deserialize` and `JsonSchema` on it, and uses the schema as the function's `parameters`. An `Option<T>` parameter is optional; everything else is required.
- **Attribute keys:**
  - `description = "..."` — the function description (optional but recommended).
  - `name = "..."` — overrides the tool name (default: the fn name). Must match `[a-zA-Z0-9_-]{1,64}`.
  - `<param> = "..."` — a description for parameter `<param>`, folded into its schema. Optional, repeatable.
- **Sync bodies** are allowed: a non-`async fn` is accepted and wrapped; the macro emits the same `Tool` impl.
- **Visibility & attributes** are preserved: the generated type inherits the function's visibility (`pub async fn add` → `pub struct add`; a private fn yields a private type), and doc comments, `#[cfg(...)]`, and other attributes on the function are forwarded to the generated item.
- **The function item is replaced by the tool value.** After expansion, `add` names the generated tool type/value, not a directly callable function. Put shared logic in a private helper if you need both a normal Rust function and a tool.

The generated type deliberately keeps the function's snake_case identifier, suppressed with `#[allow(non_camel_case_types)]`, so the ordinary call site stays `agent.tool(add)` without a separate exported constant.

Generated shape (illustrative):

```rust
#[allow(non_camel_case_types)]
pub struct add;                          // visibility inherited from the fn
impl ur::Tool for add {
    fn name(&self) -> &str { "add" }
    fn schema(&self) -> ur::ToolSchema { /* {name, description, parameters} */ }
    fn call(&self, args: ur::ToolArguments)
        -> ur::BoxFuture<'static, core::result::Result<String, String>>
    { /* deserialize args -> call body -> serialize Ok / stringify Err */ }
}
```

### The `Tool` trait

Object-safe (C-OBJECT) so a heterogeneous tool set is stored as `Arc<dyn Tool>`.

```rust
pub trait Tool: Send + Sync + 'static {
    fn name(&self) -> &str;
    fn schema(&self) -> ToolSchema;
    fn call(&self, args: ToolArguments)
        -> BoxFuture<'static, std::result::Result<String, String>>;
}

impl<T: Tool + ?Sized> Tool for std::sync::Arc<T> {
    fn name(&self) -> &str { (**self).name() }
    fn schema(&self) -> ToolSchema { (**self).schema() }
    fn call(&self, args: ToolArguments)
        -> BoxFuture<'static, std::result::Result<String, String>> { (**self).call(args) }
}
```

`Tool` is intentionally not sealed: callers may implement it manually for dynamic tools, and the macro is only the shortest path for normal Rust functions.

### `ToolSchema` and `ToolArguments`

```rust
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ToolSchema {
    pub name: String,
    pub description: Option<String>,
    /// JSON Schema for the parameters object.
    pub parameters: JsonValue,
    /// Strict (constrained-schema) mode: a hint that the provider should constrain
    /// the model's arguments to `parameters`. Default false; the exact semantics
    /// and any preconditions are provider-defined.
    pub strict: bool,
}

impl ToolSchema {
    /// Construct a non-strict schema with no description.
    /// `name` is validated when the tool is registered or sent.
    pub fn new(name: impl Into<String>, parameters: JsonValue) -> Self;

    pub fn description(self, description: impl Into<String>) -> Self;
    pub fn strict(self, strict: bool) -> Self;
}

/// Raw, unparsed tool-call arguments as delivered on the wire (a JSON string).
/// Parsing is the caller's choice — the wire delivers raw fragments we accumulate.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize), serde(transparent))]
pub struct ToolArguments(/* private String */);

impl ToolArguments {
    pub fn new(raw_json: impl Into<String>) -> Self;
    pub fn as_str(&self) -> &str;
    pub fn parse<T: serde::de::DeserializeOwned>(&self) -> Result<T, JsonError>;
    pub fn to_value(&self) -> Result<JsonValue, JsonError>;
}
impl From<String> for ToolArguments { /* ... */ }
impl From<&str> for ToolArguments { /* ... */ }
impl std::fmt::Display for ToolArguments { /* writes the raw JSON */ }
```

Under the `serde` feature `ToolArguments` (de)serializes transparently as its inner JSON string; this is what lets `Event::ToolCall` derive `Serialize`/`Deserialize` (§6).

---

## 5. Agent and Session

```rust
pub struct Agent<P: Provider> { /* private */ }

impl<P: Provider> Agent<P> {
    pub fn new(system_prompt: impl Into<String>, model: Model<P>) -> Self;

    /// Register a tool. Chainable.
    pub fn tool<T: Tool>(self, tool: T) -> Self;
    pub fn tools<T, I>(self, tools: I) -> Self
    where
        T: Tool,
        I: IntoIterator<Item = T>;

    /// Start a fresh conversation. Cheap; an agent may spawn many sessions.
    pub fn session(&self) -> Session<P>;
}
```

```rust
pub struct Session<P: Provider> { /* private */ }

impl<P: Provider> Session<P> {
    /// Send a user turn and stream the model's response, running the full agent
    /// loop (tool calls included) until the model stops. Borrows the session for
    /// the duration of the stream and commits history when the turn completes.
    pub fn send(&mut self, message: impl Into<UserMessage>) -> EventStream<'_>;

    /// The accumulated conversation, including tool turns. Read-only.
    pub fn history(&self) -> &[Message];

    /// Drop all turns after the system prompt.
    pub fn reset(&mut self);
}
```

`UserMessage` is text-only today but reserved for growth:

```rust
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct UserMessage { /* private */ }
impl UserMessage {
    pub fn as_str(&self) -> &str;
}
impl From<&str> for UserMessage { /* ... */ }
impl From<String> for UserMessage { /* ... */ }
```

`UserMessage` has no `Default`: an empty user turn is not a meaningful default message.

### `EventStream`

```rust
pub struct EventStream<'a> { /* private; borrows the Session */ }
impl<'a> Stream for EventStream<'a> {
    type Item = Result<Event>;
}
```

Dropping an `EventStream` before it yields its terminal `Event::Done` cancels the in-flight turn and rolls `Session::history()` back to the exact state it had before `send` was called. The same rollback happens if the stream ends with an error. Partial assistant text, reasoning content, tool calls, and tool results are visible as events already yielded to the caller, but they are not committed to session history unless the turn completes. Session history therefore only ever contains complete turns — including each assistant turn's retained `reasoning_content` (§8) — which makes retrying the same user turn explicit.

---

## 6. Events

```rust
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Event {
    /// Incremental assistant text.
    TextDelta { delta: String },
    /// Incremental reasoning/CoT text (thinking mode only).
    ReasoningDelta { delta: String },
    /// A fully assembled tool call, emitted once its argument fragments have been
    /// accumulated and before the tool runs.
    ToolCall { id: String, name: String, arguments: ToolArguments },
    /// The result of running a tool, emitted after `ToolCall`.
    ToolResult { id: String, name: String, output: ToolOutput },
    /// Token accounting for the most recent model turn.
    Usage { usage: Usage },
    /// The model finished a turn. Terminal for the stream unless a tool round
    /// triggers another turn (in which case more events follow).
    Done { finish_reason: FinishReason },
}
```

```rust
/// The public event form of a tool's `Result<String, String>` output.
///
/// Under `serde`, this uses a stable adjacent-tagged shape:
/// `{ "status": "ok", "content": "..." }` or
/// `{ "status": "err", "content": "..." }`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(tag = "status", content = "content", rename_all = "snake_case")
)]
pub enum ToolOutput {
    Ok(String),
    Err(String),
}

impl ToolOutput {
    pub fn from_result(output: std::result::Result<String, String>) -> Self;
    pub fn as_result(&self) -> std::result::Result<&str, &str>;
    pub fn content(&self) -> &str;
}
```

```rust
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum FinishReason {
    /// The model stopped on its own or hit a stop sequence.
    Stop,
    /// Generation reached the token limit.
    Length,
    /// Output was withheld or truncated by a content filter.
    ContentFilter,
    /// The model emitted tool calls; the loop runs them and continues.
    ToolCalls,
    /// A provider-specific terminal reason, carried verbatim.
    Other(String),
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    /// Prompt tokens served from a provider-side cache, when reported.
    pub cached_prompt_tokens: Option<u32>,
    /// Reasoning tokens, when reported (thinking mode). Included in `completion_tokens`.
    pub reasoning_tokens: Option<u32>,
}
```

`Event`, `ToolOutput`, `FinishReason`, and `Usage` derive `Serialize`/`Deserialize` under the `serde` feature; `Event::ToolCall` relies on `ToolArguments` doing the same (§4). The fields a given provider can actually populate (cache accounting, reasoning tokens, the set of `FinishReason::Other` strings it emits) are documented by that provider; absent values are `None`.

---

## 7. The agent loop (Session semantics)

`send` drives this loop, yielding `Event`s as it goes:

1. Stage the `UserMessage` in a pending history buffer.
2. Call `Provider::chat` with the pending full history, the registered tool schemas, and the model settings. Consume its `RawEvent` stream:
   - `RawEvent::TextDelta(s)` → yield `Event::TextDelta`; accumulate into the assistant turn's `content`.
   - `RawEvent::ReasoningDelta(s)` → yield `Event::ReasoningDelta`; accumulate into the assistant turn's `reasoning_content`.
   - `RawEvent::ToolCallDelta { index, id, name, arguments }` → accumulate by `index` (the first fragment carries `id`/`name`; later ones extend `arguments`).
   - `RawEvent::Done { finish_reason, usage }` → if `usage` is `Some`, yield `Event::Usage`.
3. Append the assembled assistant turn to the pending history, always including any accumulated `reasoning_content` (§8).
4. If `finish_reason == ToolCalls`:
   - For each assembled call: yield `Event::ToolCall`, look up the tool by name, run it, convert the `Result<String, String>` into `ToolOutput`, yield `Event::ToolResult`, and append a tool `Message` (`tool_call_id`, `content`) to the pending history (where `content` is the success JSON or the stringified error). An unknown tool name produces a `ToolOutput::Err` and a tool message saying so — never a panic. Malformed arguments that fail to deserialize inside a macro-generated tool follow the same path: `ToolOutput::Err`, tool message, then another model turn.
   - Go to step 2 (next model turn). Do **not** yield `Event::Done` yet.
5. Otherwise commit the pending history, yield `Event::Done { finish_reason }`, and end the stream.

Tools within a single turn run sequentially in call order. (Parallelism is a possible future addition; the seam supports parallel calls via `index`.)

The loop consumes only the normalized `RawEvent` stream and makes no assumptions about a provider's wire format, chunk ordering beyond the seam contract in §8, or how usage is reported.

---

## 8. Provider seam

The trait third-party backends implement. Object-safe; `Model<Arc<dyn Provider>>` is valid.

```rust
pub trait Provider: Send + Sync + 'static {
    /// Drive one model turn. The returned stream is the raw, normalized event
    /// sequence the Session consumes (see §7).
    fn chat(&self, request: &Request)
        -> BoxStream<'static, Result<RawEvent>>;

    /// Static facts about a model id, if known.
    fn model_spec(&self, model_id: &str) -> Option<ModelSpec>;

    /// Static non-fatal notice for a model id, if any.
    fn model_notice(&self, model_id: &str) -> Option<ModelNotice> { None }
}

/// A shared provider is itself a `Provider`; this blanket impl is what makes
/// `Model<Arc<dyn Provider>>` valid.
impl<T: Provider + ?Sized> Provider for std::sync::Arc<T> {
    fn chat(&self, request: &Request)
        -> BoxStream<'static, Result<RawEvent>> { (**self).chat(request) }
    fn model_spec(&self, model_id: &str) -> Option<ModelSpec> { (**self).model_spec(model_id) }
    fn model_notice(&self, model_id: &str) -> Option<ModelNotice> { (**self).model_notice(model_id) }
}
```

**`chat` contract.** Given the full conversation, tool schemas, and settings in `Request`, a provider yields a stream that:

- emits zero or more `TextDelta`, `ReasoningDelta`, and `ToolCallDelta` items in arrival order;
- accumulates tool calls by `index`: the first fragment for an index carries `id` and `name`, and subsequent fragments for that index extend `arguments` (a possibly-empty JSON fragment) — so callers concatenate `arguments` per index and read `id`/`name` from the first fragment;
- terminates with exactly one `Done` carrying the turn's `finish_reason` and an optional `Usage`;
- reports failures as `Err(Error)` items; the `Session` stops consuming the stream on the first error.

Because `chat` returns `BoxStream<'static, ...>` while receiving `&Request`, providers must copy or clone any request data they need before constructing the stream; the returned stream must not borrow from `request`.

Retries, authentication, timeouts, and the translation of transport/HTTP failures into `Error` are the provider's responsibility and are not observable through this trait.

**`model_spec` contract.** A pure lookup over the provider's compiled-in catalog: `Some(ModelSpec { .. })` for ids it knows, `None` otherwise. Used by `Model` to expose `context_window()`/`max_output()` and to bound `max_tokens`. It must not log or otherwise perform side effects.

**`model_notice` contract.** A pure lookup over provider metadata for non-fatal model notices: `Some(ModelNotice::Deprecated { .. })` for known deprecated ids or aliases, `None` otherwise. `Model::new` is responsible for turning the returned notice into exactly one `tracing` warning for each constructed model. Providers must not log from `model_notice`; direct calls to `model_notice` are silent and repeatable.

```rust
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Request {
    pub model: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolSchema>,
    pub settings: Settings,   // thinking, effort, max_tokens, stop, response_format, temperature, top_p
}

#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Settings {
    pub thinking: Thinking,
    pub reasoning_effort: Option<ReasoningEffort>,
    pub max_tokens: Option<u32>,
    pub stop: Vec<String>,
    pub response_format: ResponseFormat,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
}

#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum RawEvent {
    TextDelta(String),
    ReasoningDelta(String),
    ToolCallDelta { index: u32, id: Option<String>, name: Option<String>, arguments: String },
    Done { finish_reason: FinishReason, usage: Option<Usage> },
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ModelSpec { pub context_window: u32, pub max_output: u32 }

impl ModelSpec {
    pub fn new(context_window: u32, max_output: u32) -> Self;
}

#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ModelNotice {
    Deprecated { message: String },
}
```

`Settings` carries each generation setting verbatim; a provider reads the ones it supports and decides how to render them (including whether any are conditioned on `thinking`). `ur` does not interpret them beyond the local `max_tokens` check in §3.

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MessageRole { System, User, Assistant, Tool }

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: ToolArguments,
}

impl ToolCall {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        arguments: impl Into<ToolArguments>,
    ) -> Self;
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Message { /* private */ }

impl Message {
    pub fn system(content: impl Into<String>) -> Self;
    pub fn user(content: impl Into<String>) -> Self;
    pub fn assistant(
        content: Option<String>,
        reasoning_content: Option<String>,
        tool_calls: Vec<ToolCall>,
    ) -> Self;
    pub fn tool(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self;

    pub fn role(&self) -> MessageRole;
    pub fn content(&self) -> Option<&str>;
    pub fn reasoning_content(&self) -> Option<&str>;
    pub fn tool_calls(&self) -> &[ToolCall];
    pub fn tool_call_id(&self) -> Option<&str>;
}
```

`Message` is the conversation element kept in `Session::history`. An assistant message carries its optional `content`, optional `reasoning_content`, and any `tool_calls`; a tool message carries its `tool_call_id` and `content`. `Session` retains each assistant turn's `reasoning_content` in history and passes the full history — `reasoning_content` included — to `Provider::chat` on every turn, so a provider that needs prior reasoning to reconstruct a turn always has it.

---

## 9. Common trait impls

Every public type implements `Debug` (C-DEBUG). The plain-data types derive it: `Error`, `Event`, `ToolOutput`, `FinishReason`, `Usage`, `ToolSchema`, `ToolArguments`, `UserMessage`, `Request`, `RawEvent`, `ModelSpec`, `ModelNotice`, `MessageRole`, `ToolCall`, `Message`, and `Settings`. The handle and stream types — `Agent<P>`, `Model<P>`, `Session<P>`, `EventStream<'_>` — carry a manual `Debug` impl that prints an opaque, struct-named summary rather than their internals, so `Debug` holds unconditionally and does not require `P: Debug`.

Clone behavior:

- `Model<P>`, `Agent<P>`, and `Session<P>` implement `Clone where P: Clone`; this lets callers branch model settings and prepared agents.
- `EventStream<'_>` is not `Clone`.

Equality and defaults:

- `Event`, `ToolOutput`, `FinishReason`, `Usage`, `ToolSchema`, `ToolArguments`, `UserMessage`, `RawEvent`, `ModelSpec`, `ModelNotice`, `MessageRole`, `ToolCall`, and `Message` derive `PartialEq`; all except `Request`/`Settings` also derive `Eq` where shown in their definitions. `Request` and `Settings` derive `PartialEq` only because generation settings include `f32`.
- `Usage`, `Thinking`, `ResponseFormat`, and `Settings` derive `Default`.
- `ToolOutput`, `FinishReason`, `Usage`, `ToolSchema`, `ToolArguments`, `UserMessage`, `RawEvent`, `ModelSpec`, `MessageRole`, `ToolCall`, and `Message` derive `Hash`; `Request` and `Settings` do not because generation settings include `f32`.

---

## 10. Complete example

The example shows the provider-agnostic flow. Constructing a concrete `Provider` and choosing a `model_id` are the only provider-specific steps; see the documentation for the provider crate you enable.

```rust
use futures_util::StreamExt;
use serde::Serialize;

#[ur::tool(description = "Add two integers.")]
async fn add(a: i64, b: i64) -> i64 { a + b }

#[derive(Serialize)]
struct Weather { temp_c: f64, summary: String }

#[ur::tool(description = "Look up the current weather for a city.")]
async fn weather(city: String) -> Result<Weather, std::io::Error> {
    Ok(Weather { temp_c: 18.5, summary: format!("clear skies over {city}") })
}

#[tokio::main]
async fn main() -> ur::Result<()> {
    // Construct any value implementing `ur::Provider` (see your provider crate's
    // documentation), then bind it to a model id from that provider's catalog.
    let provider = /* a value implementing `ur::Provider` */;
    let model = ur::Model::new(provider, "<model-id>");

    let agent = ur::Agent::new("You are a concise assistant. Use tools when useful.", model)
        .tool(add)
        .tool(weather);

    let mut session = agent.session();
    let mut events = session.send("What is 41 + 1? Use the tool.");
    while let Some(event) = events.next().await {
        match event? {
            ur::Event::TextDelta { delta } => print!("{delta}"),
            ur::Event::ReasoningDelta { .. } => {}
            ur::Event::ToolCall { name, arguments, .. } => eprintln!("\ncall {name}({arguments})"),
            ur::Event::ToolResult { output, .. } => match output {
                ur::ToolOutput::Ok(v) => eprintln!("result: {v}"),
                ur::ToolOutput::Err(e) => eprintln!("error: {e}"),
            },
            ur::Event::Usage { usage } => eprintln!(
                "tokens: in={} (cached {}) out={}",
                usage.prompt_tokens,
                usage.cached_prompt_tokens.unwrap_or(0),
                usage.completion_tokens,
            ),
            ur::Event::Done { .. } => break,
            _ => {}
        }
    }
    Ok(())
}
```

The example uses `futures_util::StreamExt` for `.next()`, so applications using this style should depend on `futures-util`. Callers may also drive `EventStream` with the lower-level `ur::Stream` API.
