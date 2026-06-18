## Workflow

1. Read `.k/LOG.md`.
2. Do the work using the `/krust` skill. Do not use any other skills.
   - Code comments must not reference phases or the plan itself.
3. Run `cargo fmt`, `cargo clippy`, and `cargo test` for the relevant scope and fix any issues.
4. Update the phase status in this document.
5. Run a code review in a subagent. Do not use `/kreview`.
6. Log decisions, trade-offs, and deferred work in `.k/LOG.md` under a "Phase X" section.
7. Commit changes to version control following the 7 rules of great commit messages. No Co-authored-by or commit message attribution.

When testing the DeepSeek provider, use the `DEEPSEEK_API_KEY` in `.env` for live tests. Cost is not a concern.