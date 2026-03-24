Use the @Makefile targets to build, lint, format, and test the project.
Always run `make smoke-test` after `make verify` — API keys are available in `.env`.
If `.env` is missing or a key is not found, check `.env` at the project root and add any missing keys there.
When working in a worktree (`.worktrees/`), copy `.env` from the main checkout since it is gitignored and won't be present: `cp /Users/kyle/src/ur/.env .env`
Use cargo add to install dependencies -- do not modify Cargo.toml directly.
Follow the 7 rules of great commit messages. No Claude Code attribution.
Do not use branches. Work directly out of main.
This is greenfield development. No backwards compatibility concerns. Refactor as needed.