Keep this document accurate and short.

## Code

- `crates/ur` — the interactive Rust ACP client.
- `crates/ur-fake-server` — the scripted ACP server used in tests.
- `crates/ur/tests` — isolated tmux integration tests.
- `scripts/` — build and run scripts.
- `tmux.conf` — the explicitly loaded tmux configuration.
- `agents/` — agent docs, plans, work logs, reviews, and issues.

## Validation

- `make check` runs every check. Run it after changing code.
- `make check-docs` checks the Markdown. Run it after changing only docs or comments.
- `make format` formats the code and the Markdown.
- `make e2e` runs the isolated tmux tests. Run it after changing terminal behavior, which also needs
  integration tests.

Report any check that fails or is skipped.

## Ox workflow

Plans, work logs, reviews, and issues live in `agents/`.

- `/ox-plan` explores a change and writes a plan to `agents/plans/`.
- `/ox-work` implements a plan, writes a work log to `agents/work/`, and commits.
- `/ox-review` reviews code, writes a review to `agents/reviews/`, and records each finding in
  `agents/issues.csv`.
- `agents/todo.md` is the task list. High and medium severity issues are added under the task they
  affect, or as new top-level items.
- `agents/issues.csv` is the issue log. Each row has an id (`OX-NNNN`), a created time, a title, a
  severity (`low`, `medium`, `high`), the review lens that found it, a status (`unplanned`,
  `planned`, `wontfix`, `fixed`), and the review that found it. Issues found outside a review leave
  the lens and review empty. Append rows; never reorder or delete them, because `todo.md` links to
  rows by line number.

## Documentation

The code describes what the code does. Docs never restate it: no descriptions of files, functions,
fields, or UI flows, and no summaries of changes. Git history records the changes.

Most changes need no doc edits. Before editing any doc, check whether the change alters what that
doc covers. If it does not, leave the doc alone, even when the doc mentions the feature. A fact
lives in one place, never in several docs. When a doc passage is wrong, correct or delete it without
expanding it. Add a doc or a section only when asked.

- `AGENTS.md`: instructions for agents and the top-level directory map.
- `README.md`: how a user installs and uses ur. It changes when what a user does changes.
- `agents/architecture.md`: ur's components, the boundaries between them, what each owns, and the
  decisions that shape them.
- `agents/testing.md`: how to run the tests, where each kind of test goes, and test discipline.
- `agents/glossary.md`: naming rules and one-line definitions of domain terms.

Never mention "milestones", "phases", etc. in code comments or documentation (other than todo.md) --
describe the work instead.

Read before planning and changing code:

- `agents/architecture.md`
- `agents/todo.md`
- `agents/code-style.md`
- `agents/glossary.md`
- `agents/testing.md`

## Backwards Compatibility

Currently, there is none. Replace obsolete config files instead of adding migrations or versions. ur
does not read or write workspace or layout state.

## Communication

- Always describe things directly, clearly, and plainly
- Follow big idea up front and progressive disclosure
- Never use jargon, invented terms, or shorthand
- Never mix definitions or overload terms
