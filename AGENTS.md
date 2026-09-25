THIS DOCUMENT MUST BE KEPT UP TO DATE

## Code

There is no code yet. Map each file here as it is added, following the project
layout in `eng/architecture.md`.

## Validation

For changes affecting behavior, interfaces, artifacts, or builds, run full
validation: `cargo fmt --all -- --check`,
`cargo test --workspace --all-targets --all-features`,
`cargo build --workspace --all-features`,
`cargo clippy --workspace --all-targets --all-features -- -D warnings`. Add the
webview's checks here when `app/` exists. Report any skipped or failed check; do
not call partial validation complete. For documentation-only, comment-only, and
filename-only changes, use focused searches and diff inspection.

## Documentation

Use `YYYY-MM-DD-NNN-slug.md` filenames for:

- Plans in `eng/plans/`.
- Code reviews in `eng/reviews/`.

Plan reviews stay in the conversation. Do not create review documents for plans.
Include this rule explicitly when asking Claude or another agent to review a
plan.

Read before planning and changing code:

- `eng/architecture.md`
- `eng/todo.md`
- `eng/code-style.md`
- `eng/glossary.md`
- `eng/testing.md`

## Backwards Compatibility

Currently, there is none. Delete `state.json` and `gui.json` instead of adding
migrations or versions. Their directory is `$XDG_STATE_HOME/ur`, else
`~/.local/state/ur`.

## Communication

- Always describe things directly, clearly, and plainly
- Follow big idea up front and progressive disclosure
- Never use jargon, invented terms, or shorthand
- Never mix definitions or overload terms
