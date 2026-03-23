---
title: Add model metadata and info CLI command
type: feat
date: 2026-03-22
---

# Add model metadata and info CLI command

Add required metadata fields to `model-descriptor` (context window,
knowledge cutoff, pricing) and a `model info` CLI command to query
individual properties.

## Acceptance Criteria

- [ ] WIT `model-descriptor` includes five new required fields
- [ ] Google provider populates all fields for all three models
- [ ] Test/mock LLM providers populate all fields (can use placeholder values)
- [ ] `ur model info <provider/model> <property>` prints a single value
- [ ] Smoke test covers the new command
- [ ] All existing tests pass (`make verify`)

## WIT Changes

`wit/world.wit` — extend `model-descriptor`:

```wit
record model-descriptor {
    id: string,
    name: string,
    description: string,
    is-default: bool,
    settings: list<setting-descriptor>,
    // New fields — all required
    context-window-in: u32,     // max input tokens
    context-window-out: u32,    // max output tokens
    knowledge-cutoff: string,   // YYYY-MM format
    cost-in: u32,               // input price: millidollars per million tokens
    cost-out: u32,              // output price: millidollars per million tokens
}
```

**Pricing unit:** millidollars per million tokens (`u32`). $0.50/Mtok = 500,
$12/Mtok = 12000. The CLI displays as dollars by dividing by 1000.

**Knowledge cutoff:** `YYYY-MM` string (ISO 8601 partial date, sortable).

## Google Provider Data

`extensions/system/llm-google/src/lib.rs` — update to 3.1 models and
add flash-lite:

| Model | ctx_in | ctx_out | cutoff | cost_in | cost_out |
|---|---|---|---|---|---|
| gemini-3.1-flash-lite-preview | 1,048,576 | 65,536 | 2025-01 | 250 | 1500 |
| gemini-3.1-flash-preview | 1,048,576 | 65,536 | 2025-01 | 500 | 3000 |
| gemini-3.1-pro-preview | 1,048,576 | 65,536 | 2025-01 | 2000 | 12000 |

Flash-lite is $0.25/$1.50 (text input pricing). Pro uses the
<200k token tier ($2/$12).

## Test/Mock Providers

`extensions/workspace/llm-test/src/lib.rs` and
`extensions/workspace/test-extension/src/lib.rs` — add placeholder
values (e.g. `context_window_in: 1_000_000`, `cost_in: 0`).

## CLI Command

`src/cli.rs` — add variant to `ModelAction`:

```rust
/// Query a model property.
Info {
    /// Provider/model reference (e.g. "google/gemini-3.1-flash-preview").
    model_ref: String,
    /// Property name (context_window_in, context_window_out,
    /// knowledge_cutoff, cost_in, cost_out).
    property: String,
},
```

`src/model.rs` — add `cmd_info` handler:

1. Parse `model_ref` into provider/model.
2. Look up descriptor via `find_descriptor`.
3. Match property name, print value.
   - `cost_in`/`cost_out`: print as dollars with two decimal places
     (e.g. `0.50`, `12.00`).
   - `context_window_in`/`context_window_out`: print raw u32.
   - `knowledge_cutoff`: print string as-is.
4. Error on unknown property.

`src/main.rs` — wire `ModelAction::Info` to `cmd_info`.

## Smoke Test

`scripts/smoke_test/test_model_roles.py` — add assertions:

```python
# Query each property for a known model
result = h.run("model", "info", "google/gemini-3.1-flash-preview", "cost_in")
assert "0.50" in result.stdout

result = h.run("model", "info", "google/gemini-3.1-flash-preview", "context_window_in")
assert "1000000" in result.stdout

result = h.run("model", "info", "google/gemini-3.1-flash-preview", "knowledge_cutoff")
assert "2025-01" in result.stdout

# Error on unknown property
h.run_err("model", "info", "google/gemini-3.1-flash-preview", "nonexistent")

# Error on unknown model
h.run_err("model", "info", "google/nonexistent", "cost_in")
```

## Implementation Order

1. WIT record change (breaks all providers until updated)
2. Google provider — populate real data
3. Test/mock providers — populate placeholder data
4. `make check` to verify all extensions compile
5. CLI `Info` variant + `cmd_info` handler + wiring
6. Unit tests for `cmd_info` in `src/model.rs`
7. Smoke test additions
8. `make verify`
