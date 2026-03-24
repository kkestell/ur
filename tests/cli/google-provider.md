# Google Provider

Live completions against Google Gemini models. Requires
`GOOGLE_API_KEY` in the environment (source `.env` from repo root).

## Prerequisites

Requires the workspace test-extension for tool calling:

```bash
cargo build --manifest-path extensions/workspace/test-extension/Cargo.toml \
  --target wasm32-wasip2 --release
mkdir -p "$W/.ur/extensions/test-extension"
cp extensions/workspace/test-extension/target/wasm32-wasip2/release/test_extension.wasm \
  "$W/.ur/extensions/test-extension/"
ur -w "$W" extension enable test-extension
```

## Flash — low thinking

```bash
ur -w "$W" role set default google/gemini-3-flash-preview
ur -w "$W" extension config llm-google set gemini-3-flash-preview.thinking_level low
ur -w "$W" extension config llm-google set gemini-3-flash-preview.max_output_tokens 1024
ur -w "$W" run "What is the weather in Paris, and should I wear a coat?"
```

Should succeed with a natural language response. Transient 5xx or
rate-limit errors may warrant a retry.

## Flash — high thinking

```bash
ur -w "$W" extension config llm-google set gemini-3-flash-preview.thinking_level high
ur -w "$W" extension config llm-google set gemini-3-flash-preview.max_output_tokens 2048
ur -w "$W" run "What is the weather in Paris, and should I wear a coat?"
```

Should succeed.

## Pro — medium thinking

```bash
ur -w "$W" role set default google/gemini-3.1-pro-preview
ur -w "$W" extension config llm-google set gemini-3.1-pro-preview.thinking_level medium
ur -w "$W" extension config llm-google set gemini-3.1-pro-preview.max_output_tokens 1536
ur -w "$W" run "What is the weather in Paris, and should I wear a coat?"
```

Should succeed.

## Pro — high thinking

```bash
ur -w "$W" extension config llm-google set gemini-3.1-pro-preview.thinking_level high
ur -w "$W" extension config llm-google set gemini-3.1-pro-preview.max_output_tokens 3072
ur -w "$W" run "What is the weather in Paris, and should I wear a coat?"
```

Should succeed.

## Flash Lite — minimal thinking

```bash
ur -w "$W" role set default google/gemini-3.1-flash-lite-preview
ur -w "$W" extension config llm-google set gemini-3.1-flash-lite-preview.thinking_level minimal
ur -w "$W" extension config llm-google set gemini-3.1-flash-lite-preview.max_output_tokens 768
ur -w "$W" run "What is the weather in Paris, and should I wear a coat?"
```

Should succeed.

## Teardown

```bash
ur -w "$W" extension disable test-extension
```
