# Errors

## The `Error` enum
The full `#[non_exhaustive]` variant list with a one-line meaning for each.

## Variant reference
A subsection per variant describing its fields and when it surfaces.

### `Auth` / `InsufficientFunds`
Parameterless auth/credit failures determined by status alone.

### `BadRequest` / `InvalidParams`
Rejected-request variants carrying a provider message.

### `RateLimited`
Carries an optional `retry_after` duration.

### `Server`
Catch-all for retryable server statuses with `status` + `message`.

### `Transport` / `Decode`
Wrapped lower-level errors whose `.source()` is exposed.

### `Config`
Pre-request misconfiguration surfaced before any network call.

## When errors surface
Which layer produces each error: before request, during streaming, or during tool dispatch.

## Tool errors vs turn errors
How malformed arguments or unknown tools become `ToolOutput::Err` without failing the turn.

## Rollback on error
How provider errors and early stream drops discard the pending turn and preserve committed history.

## Error sources
Which variants expose a `.source()` and how to downcast wrapped transport/decode causes.

## Handling guidance
Idiomatic matching patterns and retry-vs-fail decisions per variant.
