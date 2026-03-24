# Structured Logging, File Sinks, and TUI Verbosity

## How Might We

How might we make logging across `ur` and `ur-tui` comprehensive, structured,
and boring to debug, without polluting user-facing terminal output or making
the TUI behave surprisingly?

## Why This Approach

The repo already has a first pass at `tracing` in `ur`, but coverage is still
thin and `ur-tui` has no logging bootstrap yet. This brainstorm focuses on a
single mental model that works across both binaries:

- every run produces a structured log artifact
- `-v` means "show me more diagnostic detail"
- UI output and diagnostic output stay intentionally separate

That keeps the product predictable for users and gives us something supportable
when debugging session, tool, extension, provider, or TUI issues later.

## Repo Context

- `ur` already supports `-v/--verbose` and tests expect verbose logs on
  `stderr`.
- `ur-tui` runs on the alternate screen and currently has no verbose flag or
  logging initialization.
- `UR_ROOT` already defines the app root, with `~/.ur` as the default.
- Session lifecycle logging exists in `src/session.rs`, but coverage is not yet
  consistent across app startup, workspace loading, extension discovery, TUI
  lifecycle, and error paths.

## SCAMPER Insights

- **Substitute:** replace ad hoc terminal-oriented debugging with per-run file
  sinks.
- **Combine:** share one logging bootstrap policy between `ur` and `ur-tui`,
  with frontend-specific console behavior.
- **Adapt:** follow the existing `UR_ROOT` pattern, so logs live at
  `$UR_ROOT/logs` and default to `~/.ur/logs`.
- **Eliminate:** avoid writing debug logs "behind" the alternate screen.
- **Reverse:** treat the log file, not the terminal, as the durable debugging
  surface.

## Assumptions

Validated from the prompt and current repo state:

- The goal is stronger structured logging coverage across both binaries, not
  just replacing a few missing print statements.
- The primary use case is post-mortem debugging after failures.
- We are still early in development and there are no external users yet.
- The preferred solution is boring and predictable over clever or flashy.
- A per-run log artifact sounds acceptable and likely desirable.
- `~/.ur/logs` should really mean `$UR_ROOT/logs`, with `~/.ur/logs` as the
  default path.
- `ur-tui` should not sacrifice terminal correctness just to make verbose logs
  visible in shell scrollback.
- Errors should always appear in logs.
- Regular logs and verbose logs should follow simple, easy-to-explain rules.
- Verbose logs should usually get us close to diagnosing a failure without
  needing to reproduce it immediately.
- Verbose logs may include full prompt, tool-argument, and tool-result bodies
  by default.
- Retention can be manual for now; keep all logs.
- Timestamped per-run filenames are sufficient; no convenience pointer such as
  `latest.log` is needed.
- Failure exits should automatically print the log file path in both `ur` and
  `ur-tui`.
- The boring, expected level split is: normal file logs at `info`, verbose file
  logs at `debug`.

## Constraints

- `ur` already establishes a user expectation: `-v` produces console-visible
  diagnostic output on `stderr`.
- `ur-tui` uses the alternate screen, so normal shell-output assumptions do not
  map cleanly.
- Logging may include prompts, tool arguments, tool output, provider responses,
  and local paths; privacy/noise boundaries matter.
- The most valuable next step is a shared policy, not a TUI-specific observability
  subsystem.
- Since there are no external users yet, we can bias somewhat toward diagnostics
  over minimalism, but we still want stable conventions.

## Comparable Tool Patterns

Current agentic coding assistants do not seem to converge on one universal
logging model, but a few patterns show up repeatedly:

- **Claude Code** exposes both `--verbose` and `--debug`, with `--verbose`
  described as showing full turn-by-turn output and `--debug` supporting
  category filtering. It also persists sessions for later resume. This suggests
  a split between richer console output and deeper internal diagnostics, rather
  than a single monolithic "debug mode."
- **Aider** leans on persistent history and transcript artifacts such as chat
  history files and optional LLM history files. This is durable, but it is more
  conversation-oriented than structured runtime observability.
- **Gemini CLI** is the clearest on structured observability: it documents
  structured events with a session identifier, event names, and configurable
  prompt logging / debug settings.

Takeaway:

- `--verbose` for richer console detail is common.
- Persistent local artifacts are common.
- Structured event logging is good practice, but many tools do not make it the
  primary local debugging surface.

For `ur`, a per-run structured log file is therefore not weird; it is arguably
more disciplined than the norm, and it matches the post-mortem-debugging goal
well.

## Raw Options

1. Single append-only global log file for all runs.
2. One structured log file per run, no console mirroring.
3. One structured log file per run, plus `-v` console mirroring where it makes
   sense.
4. Keep `-v` in `ur-tui` and print logs to the primary screen while the TUI uses
   the alternate screen.
5. Add an in-app TUI debug pane instead of relying on shell-visible verbose
   output.

## Pre-Mortem

### Option 1: Single global log file

- Concurrent runs interleave and become hard to reason about.
- The file grows forever and turns support/debugging into grep archaeology.

### Option 3: Per-run files plus selective console mirroring

- If the rules differ too much between binaries, users may get confused about
  what `-v` means.
- If we mirror too much to the console, we reintroduce noisy UX and accidental
  prompt/tool leakage.

### Option 4: Print behind the alternate screen

- Terminal behavior feels magical and fragile.
- Users only discover what happened after exit, and the behavior will vary by
  shell, multiplexer, and crash path.

### Option 5: In-app debug pane

- Scope expands quickly from "boring logging" to "designing a debugger."
- We invest in UI chrome before we have a stable logging schema.

## Best Approaches

### Recommended: Per-run file logs always, `-v` raises visibility but not policy

Each process run gets its own structured log file in `$UR_ROOT/logs`, named
with timestamp, binary, and pid/session identifier. Logging always writes to
that file. `-v` increases verbosity and mirrors logs to the console only in
frontends where console mirroring is natural.

Pros:
- Most boring and supportable model.
- No interleaving between concurrent runs.
- Works for both CLI and TUI without abusing terminal behavior.
- Preserves shell UX for `ur` and keeps `ur-tui` clean.

Cons:
- Always-on file logging creates disk retention questions.
- `ur-tui -v` will not look identical to `ur -v` unless we document the sink
  difference clearly.

Failure modes:
- Logs become too chatty if we do not define level boundaries carefully.
- Users may miss the log file unless the path is surfaced on failure or via an
  explicit status line/message.

Best suited when:
- We want predictable diagnostics now and richer TUI debugging later.

Design quality lens:
- **SRP:** a shared logging bootstrap owns sink setup; app/session/TUI code just
  emits events.
- **OCP / DIP:** binaries depend on a logging abstraction/policy, not sink
  details.
- **YAGNI / KISS:** no TUI log pane, no special terminal tricks.
- **Value Objects:** `RunLogPath`, `RunId`, and `FrontendKind` are worth naming.
- **Complexity:** low accidental complexity; one durable sink, optional mirror.

Object stereotypes:
- `LoggingConfig`: Information Holder
- `LoggingBootstrap`: Service Provider
- `RunContext`: Information Holder
- `ConsoleMirrorPolicy`: Structurer

### Alternative: Per-run logs only when `-v` is set

Only verbose runs create a file, and `-v` remains the main opt-in for all
logging. This minimizes disk usage and keeps logging conceptually tied to a
debug mode.

Pros:
- Simpler retention story.
- Easier to explain if we want logging to stay explicitly opt-in.

Cons:
- Harder to debug failures users forget to rerun with `-v`.
- Weakens the "every run has an artifact" support story.

Failure modes:
- The most important failures happen in non-verbose runs and leave no trace.
- Teams start asking for ad hoc extra printing again.

Best suited when:
- Disk sensitivity matters more than post-hoc diagnosability.

Design quality lens:
- **SRP:** clean, but pushes too much responsibility onto the user.
- **OCP / DIP:** still extensible.
- **YAGNI / KISS:** simple, but maybe too simple for a diagnostics-first tool.
- **Value Objects:** same as above.
- **Complexity:** low implementation complexity, higher operational complexity.

Object stereotypes:
- `VerboseLoggingGate`: Controller
- `RunContext`: Information Holder

### Future-facing: Per-run files now, optional in-app TUI diagnostics later

Adopt the recommended file-first policy now, and reserve a future TUI-only
feature for an explicit debug pane or log modal if the team later wants live
inspection during a session.

Pros:
- Keeps current scope small.
- Leaves room for richer TUI observability without overloading `-v`.

Cons:
- Does not satisfy any desire for live, in-session log inspection today.
- Adds a documented future seam that may or may not ever be needed.

Failure modes:
- The future hook becomes an excuse to postpone good file log coverage.
- We prematurely optimize the schema for a UI that does not exist yet.

Best suited when:
- We want a stable base now and optional tooling later.

Design quality lens:
- **SRP:** strong separation between logging and UI.
- **OCP / DIP:** easiest approach to extend later.
- **YAGNI / KISS:** still grounded if we stop at the file-first phase.
- **Value Objects:** add `LogViewMode` only if the future feature becomes real.
- **Complexity:** phased and controlled.

Object stereotypes:
- `LoggingBootstrap`: Service Provider
- `TuiDiagnosticsView` (future): Interfacer

## Recommendation

Choose the first approach.

### Key decisions

- Give each `ur` and `ur-tui` process run its own log file.
- Put logs under `$UR_ROOT/logs`, defaulting to `~/.ur/logs`.
- Make the per-run log file the primary debugging artifact.
- Keep `-v/--verbose` in `ur-tui` for consistency, but define it as a verbosity
  control, not "write behind the alternate screen."
- Let `ur -v` continue mirroring human-readable diagnostics to `stderr`.
- Let `ur-tui -v` increase file log detail and, at most, print a short log-path
  note after the terminal is restored or when the app exits with an error.
- Log files should default to normal lifecycle breadcrumbs plus warnings/errors;
  `-v` should raise the file detail level enough to diagnose most issues.
- Keep all log files for now; do not add retention cleanup in the first cut.
- Use timestamped per-run filenames only; do not create a `latest.log` alias or
  last-run pointer.
- On failures, automatically surface the log path on exit in both binaries.
- Use `info` for normal per-run file logs and `debug` for verbose per-run file
  logs.
- Log files use standard `tracing_subscriber::fmt` human-readable format, not
  JSON. "Structured" means tracing spans and fields, not a special serialization
  format.
- Filename pattern: `{binary}-{timestamp}-{pid}.log`, e.g.
  `ur-2026-03-24T14-56-08-12345.log`.

## Proposed Level Rules

### Regular logs

Regular per-run logs should answer: "what happened?" without drowning the run
in payload detail.

Level: `info`

Include by default:

- startup and shutdown
- workspace / root / config resolution
- manifest / extension discovery summaries
- provider selection and model / role resolution
- session open / turn start / turn end / interruption
- tool call boundaries and outcome status
- warnings and errors
- timings, counts, identifiers, and state transitions

Exclude by default:

- full prompt bodies
- full streamed text deltas
- full tool argument JSON
- full tool result payloads
- every fine-grained UI event or keypress

### Verbose logs

Verbose logs should answer: "why did it fail?" often enough that a rerun is not
the first debugging step.

Level: `debug`

Add at verbose level:

- richer provider / extension initialization detail
- full prompt bodies
- full tool argument bodies
- full tool result bodies
- request / response-ish boundaries and intermediate decisions
- more detailed TUI lifecycle and background-task transitions
- lower-level breadcrumbs that would be too noisy in regular logs

Still avoid by default:

- secret values
- raw tokens / every streaming delta unless there is a very strong reason
- unbounded payload duplication

### Error rule

Errors should always be logged. When a failure occurs, the log should include:

- what operation failed
- enough identifiers to correlate the failure
- the error chain / message
- the phase of execution
- the log path surfaced to the operator on exit where practical

## Coverage Guidance

Good coverage should include:

- app startup and shutdown
- workspace open/root resolution
- manifest discovery and extension enablement state
- provider/key/config resolution failures
- session open, turn start, turn completion, interruption, and errors
- LLM call boundaries, tool call boundaries, approvals, and tool results
- TUI lifecycle events such as startup, terminal setup/restore, command
  submission, modal open/close, and background-turn state changes

What should usually not be logged at high-volume by default:

- every keypress
- every text delta token
- full secret values
- redundant copies of large tool outputs or full prompts at `info`

## Failure Modes

- Sensitive prompt or tool data leaks into logs if field selection is too
  casual.
- Log retention becomes messy without a simple cleanup policy.
- Too much event volume makes the logs expensive to read and too little makes
  them useless.
- `ur` and `ur-tui` drift into separate semantics for `-v`.

## Open Questions

None blocking for planning. Future refinements can revisit payload truncation,
retention, or more specialized debug modes if real usage shows a need.
