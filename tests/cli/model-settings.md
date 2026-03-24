# Model Settings

Extension config set/get/list and validation of setting constraints
(enums, integer bounds, readonly fields).

## Configure roles and settings

```bash
ur -w "$W" role set default google/gemini-3-flash-preview
ur -w "$W" role set fast google/gemini-3.1-pro-preview
ur -w "$W" role set lite google/gemini-3.1-flash-lite-preview
```

Should all succeed.

```bash
ur -w "$W" extension config llm-google set gemini-3-flash-preview.thinking_level minimal
ur -w "$W" extension config llm-google set gemini-3.1-pro-preview.thinking_level low
ur -w "$W" extension config llm-google set gemini-3.1-flash-lite-preview.thinking_level minimal
ur -w "$W" extension config llm-google set gemini-3.1-pro-preview.max_output_tokens 4096
ur -w "$W" extension config llm-google set gemini-3-flash-preview.max_output_tokens 2048
```

Should all succeed.

```bash
ur -w "$W" extension config llm-google list
```

Output should include `thinking_level` and `max_output_tokens`.

## Error cases: invalid settings

```bash
ur -w "$W" extension config llm-google set nonexistent_key 42
```

Should error — key doesn't match any model.setting pattern.

```bash
ur -w "$W" extension config llm-google set gemini-3-flash-preview.thinking_level ultra
```

Should error — `ultra` is not a valid enum value.

```bash
ur -w "$W" extension config llm-google set gemini-3.1-pro-preview.thinking_level minimal
```

Should error — `minimal` is not allowed for this model.

```bash
ur -w "$W" extension config llm-google set gemini-3-flash-preview.max_output_tokens 0
```

Should error — below minimum bound.

```bash
ur -w "$W" extension config llm-google set gemini-3-flash-preview.context_window_in 500000
```

Should error — readonly setting.

## Verify config.toml

Read `~/.ur/config.toml`. It should contain `thinking_level` and
`max_output_tokens` entries reflecting the values set above.
