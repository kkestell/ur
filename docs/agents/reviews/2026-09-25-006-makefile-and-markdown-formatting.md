# Makefile and Markdown formatting

## Scope and coverage

Reviewed commit `251bfde`: the new `Makefile` and `dprint.json`, the `AGENTS.md` changes that
describe them, and the reformatting of every Markdown file under `docs/agents/`. Every word-level
change in the Markdown diff was read, and the contents of fenced code blocks were compared before
and after, to confirm the reformatting changed only line wrapping.

Lenses: correctness and documentation.

## Fixed

- **Reformatting stripped meaningful spaces from code spans**
  (`docs/agents/plans/2026-09-25-005-status-permissions-and-workspaces.md:151`,
  `docs/agents/work/2026-09-25-004-daemon-hosts-acp-sessions.md:73`): dprint trims leading and
  trailing spaces inside code spans. In the plan, the session line of `ur ls` lost its two leading
  spaces, and so did the unread marker, so the plan no longer said that `ur ls` indents each session
  under its workspace, which `crates/ur/src/cli/ls.rs` does. In the work log, the streamed chunks
  lost the trailing space that showed where each chunk ended. Restoring the old text would be
  trimmed again by the next `make format-docs`, so both passages now say the spacing in words.

## Findings

No open findings.

## Checks run

- `git diff --word-diff` over the Markdown files — the only token changes were the `AGENTS.md`
  additions and the trimmed code spans above.
- Comparison of fenced code block contents before and after the commit — identical.
- `dprint output-file-paths` — covers `AGENTS.md` and `docs/agents/` only.
- `make check` — passed.

## Verdict

The Makefile and dprint config work as `AGENTS.md` describes. The one content change the formatter
introduced is fixed.
