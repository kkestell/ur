# Agent Turn

Deterministic agent turn using the test/echo LLM provider. This
test does not hit any external API.

## Prerequisites

Requires workspace test extensions. Build and place them first:

```bash
cargo build --manifest-path extensions/workspace/test-extension/Cargo.toml \
  --target wasm32-wasip2 --release
cargo build --manifest-path extensions/workspace/llm-test/Cargo.toml \
  --target wasm32-wasip2 --release

mkdir -p "$W/.ur/extensions/test-extension"
mkdir -p "$W/.ur/extensions/llm-test"
cp extensions/workspace/test-extension/target/wasm32-wasip2/release/test_extension.wasm \
  "$W/.ur/extensions/test-extension/"
cp extensions/workspace/llm-test/target/wasm32-wasip2/release/llm_test.wasm \
  "$W/.ur/extensions/llm-test/"
```

## Run

```bash
ur -w "$W" extension enable test-extension
ur -w "$W" extension enable llm-test
ur -w "$W" role set default test/echo
```

Should all succeed.

```bash
ur -w "$W" -v run "Hello, please greet the world"
```

Should succeed. The echo provider mirrors the input back. Verbose
flag should show session event tracing.

## Teardown

```bash
ur -w "$W" extension disable llm-test
ur -w "$W" extension disable test-extension
ur -w "$W" role set default google/gemini-3-flash-preview
```

Should all succeed. Restores default state.
