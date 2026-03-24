# Model Roles

Role listing, assignment, and validation.

## List and get roles

```bash
ur -w "$W" role list
```

Should succeed. Output should include `google/gemini-3-flash-preview`.

```bash
ur -w "$W" role get default
ur -w "$W" role get fast
```

Both should succeed and print the current role mapping.

## Extension config list

```bash
ur -w "$W" extension config llm-google list
```

Should succeed. Output should contain `thinking_level`,
`max_output_tokens`, `context_window_in`, and at least one setting
marked `(readonly)`.

## Set and verify roles

```bash
ur -w "$W" role set default google/gemini-3-flash-preview
ur -w "$W" role get default
```

Should succeed. Get should reflect the assignment.

```bash
ur -w "$W" role set fast google/gemini-3.1-pro-preview
ur -w "$W" role set lite google/gemini-3.1-flash-lite-preview
ur -w "$W" role list
```

List output should show:
- `fast` mapped to `google/gemini-3.1-pro-preview`
- `lite` mapped to `google/gemini-3.1-flash-lite-preview`

```bash
ur -w "$W" role set default google/gemini-3.1-pro-preview
ur -w "$W" role set default google/gemini-3-flash-preview
```

Should succeed — swapping default back and forth.

## Error cases: invalid role targets

```bash
ur -w "$W" role set default fake/nonexistent
```

Should error — unknown provider.

```bash
ur -w "$W" role set default invalid-no-slash
```

Should error — invalid model ref format.

```bash
ur -w "$W" role set default google/nonexistent-model
```

Should error — unknown model under known provider.

## Extension config get (readonly metadata)

```bash
ur -w "$W" extension config llm-google get gemini-3-flash-preview.context_window_in
```

Output should contain `1048576`.

```bash
ur -w "$W" extension config llm-google get gemini-3-flash-preview.knowledge_cutoff
```

Output should contain `2025-01`.

```bash
ur -w "$W" extension config llm-google get gemini-3-flash-preview.context_window_out
```

Output should contain `65536`.

```bash
ur -w "$W" extension config llm-google get gemini-3-flash-preview.cost_in
```

Output should contain `500`.

```bash
ur -w "$W" extension config llm-google get nonexistent
```

Should error — unknown setting key.
