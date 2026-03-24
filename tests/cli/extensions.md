# Extensions

Extension discovery, inspection, enable/disable, and slot constraint
enforcement.

## List and inspect

```bash
ur -w "$W" extension list
```

Should list system extensions (session-jsonl, llm-google,
llm-openrouter, compaction-llm) all enabled.

```bash
ur -w "$W" extension inspect session-jsonl
ur -w "$W" extension inspect llm-google
ur -w "$W" extension inspect compaction-llm
```

Each should succeed with identity, slot, and checksum.

## Error cases: inspect/enable/disable

```bash
ur -w "$W" extension inspect nonexistent
```

Should error — unknown extension.

```bash
ur -w "$W" extension disable llm-google
```

Should succeed — llm-provider is AtLeastOne and llm-openrouter
remains.

```bash
ur -w "$W" extension disable llm-openrouter
```

Should error — last provider of required llm-provider slot.

```bash
ur -w "$W" extension enable llm-google
```

Should succeed — re-enable for subsequent tests.

```bash
ur -w "$W" extension disable compaction-llm
```

Should error — sole provider of required ExactlyOne slot.

```bash
ur -w "$W" extension disable session-jsonl
```

Should error — sole provider of required ExactlyOne slot.

```bash
ur -w "$W" extension enable nonexistent
ur -w "$W" extension disable nonexistent
```

Both should error — unknown extension.

## Workspace extensions

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

```bash
ur -w "$W" extension inspect test-extension
```

Should succeed — found via workspace tier.

```bash
ur -w "$W" extension enable test-extension
```

Should succeed.

```bash
ur -w "$W" extension enable test-extension
```

Should error — already enabled.

```bash
ur -w "$W" extension enable llm-test
ur -w "$W" extension list
```

Should show test-extension and llm-test enabled alongside system
extensions.

```bash
ur -w "$W" extension disable llm-test
ur -w "$W" extension disable test-extension
ur -w "$W" extension list
```

Should show only system extensions enabled.
