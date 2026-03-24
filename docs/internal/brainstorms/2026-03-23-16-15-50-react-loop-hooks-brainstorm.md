# ReAct Loop Extension Hooks

**Date:** 2026-03-23
**Status:** Brainstorm complete, ready for planning

## What We're Building

A middleware/hook system that lets extensions intercept and interact with every
stage of the ReAct loop. Extensions can observe, validate, transform, or reject
at each hook point. This enables observability, policy enforcement, and behavior
modification without changing the host or other extensions.

## Why This Approach

Extensions are WASM components communicating via WIT interfaces. The hook system
must fit within this model: statically typed interfaces, value-passing across
the sandbox boundary, and host-driven orchestration.

## Key Decisions

### 1. Ordering Model: User-Defined, Per-Hook-Point, Persisted

**Decision:** The user controls hook execution order independently for each hook
point. Ordering is persisted in the per-workspace manifest (~/.ur/workspaces/).

**Why:** Pipeline phases (validate -> transform -> observe) don't solve the core
problem — two extensions modifying the same data still need an order. Phases
also artificially restrict what an extension can do. User-defined ordering is
honest: ordering is a user concern, not something the system can infer.
Per-hook-point ordering (not global) because an extension's priority varies by
context — a PII redactor should run first in before-tool but order doesn't
matter in after-completion.

**Lifecycle:**
- **First run in workspace:** Default order = discovery order (system -> user ->
  workspace tier). Persisted to manifest immediately.
- **Subsequent runs:** Order loaded from manifest. Stable across restarts.
- **New extension enabled:** Appended to end of each hook chain it subscribes to.
- **Extension disabled:** Stays in manifest ordering with disabled flag. Skipped
  at runtime. Re-enabling restores its previous position.
- **User reorder:** CLI command to reorder, persisted to manifest.

### 2. Hook Registration: Extension-Declared via WIT Exports

**Decision:** Extensions declare which hooks they support by exporting specific
WIT interfaces. The host auto-discovers capabilities by inspecting exports
(same pattern as existing slot detection in slot.rs).

**Why:** Consistent with existing architecture. The user controls ordering and
enable/disable, but not assignment. Extensions can't be assigned to hook points
they don't implement.

### 3. Hook Points (9 total)

| Hook                     | Input                                      | Can Mutate                        | Can Reject |
|--------------------------|--------------------------------------------|-----------------------------------|------------|
| **before-completion**    | messages, model, settings, tools           | messages, model, settings, tools  | yes        |
| **after-completion**     | messages, model, response                  | response                          | no         |
| **before-tool**          | tool name, args, call ID                   | args                              | yes        |
| **after-tool**           | tool name, args, call ID, result           | result                            | no         |
| **before-session-load**  | session ID                                 | —                                 | yes        |
| **after-session-load**   | session ID, messages                       | messages                          | no         |
| **before-session-append**| session ID, message                        | message                           | yes        |
| **before-compaction**    | messages                                   | messages                          | yes        |
| **after-compaction**     | original messages, compacted               | compacted                         | no         |

**Note:** "after" hooks can mutate the *output* but not the *input* (it already
happened). They cannot reject (the action is done).

### 4. Reentrancy: complete-raw() Only

**Decision:** Add `host::complete-raw()` to WIT that never triggers hooks. Hooks
that need to make LLM calls use this. Regular `host::complete()` always runs the
full hook chain.

**Why:** Most hook-initiated completions are meta-calls (classification,
validation) that should not go through the hook chain. `complete-raw()` is
simple, predictable, and makes recursion impossible. Can be relaxed later if a
real use case demands hook-aware inner completions.

### 5. Return Types: Explicit Variants

**Decision:** Each before-hook returns a three-way variant:

```wit
variant before-tool-result {
    pass,
    modified(modified-tool-call),
    rejected(string),
}
```

**Why:** Explicit intent — `pass` means "I don't care, skip serialization,"
`modified` means "I changed something," `rejected` means "abort." The host can
optimize pass-through (no unnecessary serialization across the WASM boundary).

### 6. Streaming Hooks: Deferred

**Decision:** No per-chunk streaming hooks in the initial design. Hooks see the
final assembled response in `after-completion`. Leave room in the WIT to add
an `on-chunk` hook interface later.

**Why:** Per-chunk hooks multiply WASM boundary crossings by (chunks x
extensions). The complexity and latency implications are significant. Most use
cases (logging, policy, transformation) work fine on the final response. Revisit
when a concrete use case demands real-time chunk intervention.

## Open Questions

### Rejection Semantics
When a before-tool hook rejects a tool call, what does the LLM see?
- Option A: A ToolResult with an error message ("tool call rejected by policy: reason")
- Option B: The tool call is silently removed from the response
- Option C: The entire turn is aborted

Likely answer: Option A — the LLM can reason about the rejection and try a
different approach. But this needs concrete design.

### After-Hook Mutation Scope
`after-completion` can mutate the response — but what does "mutate" mean here?
Can a hook rewrite the assistant's text? Add/remove tool calls from the
response? This needs precise scoping per hook point.

### Config Schema
The exact config.toml schema for hook ordering. How does it interact with the
existing extension enable/disable mechanism? Is ordering per-workspace or global?

### WIT Interface Granularity
One WIT interface per hook point (9 interfaces) vs. a single hook interface
with an event discriminant? The former is more type-safe; the latter is more
flexible. Given the decision for explicit variants, per-hook-point interfaces
are more consistent.

### ~~Slot Model Interaction~~ RESOLVED
Hooks are orthogonal to slots. An extension has a primary slot (or none for
hook-only extensions) plus zero or more hook capabilities. Enable/disable
applies to the whole extension — you can't enable an extension's LLM slot but
disable its hooks independently. The manifest stores hook chain ordering
alongside the existing extension entries, with disabled extensions preserved in
position but skipped at runtime.

### Performance Budget
How many WASM boundary crossings per turn is acceptable? With 9 hook points and
N extensions, worst case is 9N crossings per turn (plus tool iterations). Need
benchmarks to understand if this is a concern.
