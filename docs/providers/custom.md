# Custom Providers

## When to write a custom provider
Motivation: a backend `ur` doesn't ship, offline/test doubles, or a local model server.

## The `Provider` trait
The three methods (`chat`, `model_spec`, `model_notice`) and their contracts.

## `chat` and `RawEvent`
Returning a `BoxStream<Result<RawEvent>>` and the normalized event variants to emit.

## `Request`, `Message`, and `Settings`
The records your `chat` receives and how to read history, tools, and settings from them.

## `ModelSpec` and `ModelNotice`
What catalog metadata and deprecation notices to return, if any.

## A worked example
The scripted/echo provider pattern (as in the `custom` example) running offline.

## Implementation notes
Object safety behind `Arc`, streaming-only contract, `BoxStream`/`BoxFuture` aliases, and `Send + Sync + 'static` bounds.
