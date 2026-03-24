# OpenRouter Provider

Live completions via OpenRouter. Requires `OPENROUTER_API_KEY` in
the environment (source `.env` from repo root).

## Prerequisites

Requires the workspace test-extension for tool calling. Google
provider must be disabled so OpenRouter is the only llm-provider:

```bash
cargo build --manifest-path extensions/workspace/test-extension/Cargo.toml \
  --target wasm32-wasip2 --release
mkdir -p "$W/.ur/extensions/test-extension"
cp extensions/workspace/test-extension/target/wasm32-wasip2/release/test_extension.wasm \
  "$W/.ur/extensions/test-extension/"
ur -w "$W" extension enable test-extension
ur -w "$W" extension disable llm-google
```

## OpenRouter tool-calling flow

```bash
ur -w "$W" role set default openrouter/qwen/qwen3.5-9b
ur -w "$W" role get default
ur -w "$W" extension config llm-openrouter list
ur -w "$W" run "What is the weather in Paris, and should I wear a coat?"
```

All should succeed. The run may need a retry on transient 5xx or
rate-limit errors.

## Teardown

```bash
ur -w "$W" extension enable llm-google
ur -w "$W" extension disable test-extension
```
