# Streaming

## The streaming model
`session.send()` returns an `EventStream`, a `Stream<Item = Result<Event>>` borrowing the session.

## Consuming a stream
The `while let Some(event) = stream.next().await` pattern and matching on `Event`.

## Event reference
The complete `Event` enum and what each variant carries.

### `TextDelta`
Incremental assistant text.

### `ReasoningDelta`
Incremental reasoning/thinking text.

### `ToolCall`
A fully assembled tool call ready to dispatch.

### `ToolResult`
The output (ok or err) of a completed tool call.

### `Usage`
Token accounting reported at the end of a model turn.

### `Done`
The terminal event for a whole user turn; commits history.

## Event ordering
The order events arrive within a turn and across multiple tool rounds.

## Multi-turn tool loops
How `ToolCalls` triggers another provider turn within the same `send`.

## `FinishReason`
The variants (`Stop`, `Length`, `ContentFilter`, `ToolCalls`, `Other`) and when each appears.

## `Usage`
The token fields including optional `cached_prompt_tokens` and `reasoning_tokens`.

## `ToolOutput`
The ok/err result type and its `as_result()` / `content()` helpers.

## Lifetime and cancellation
The stream borrows the session; dropping it early rolls back the pending turn.
