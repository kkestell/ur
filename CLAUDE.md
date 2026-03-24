Use the @Makefile targets to build, lint, format, and test the project.
CLI integration tests live in `tests/cli/` as markdown runbooks. They assume `ur` is installed (`make install`).
API keys are available in `.env` at the project root.
If `.env` is missing or a key is not found, add any missing keys there.
When working in a worktree (`.worktrees/`), copy `.env` from the main checkout since it is gitignored and won't be present: `cp /home/kyle/src/ur/.env .env`
Use cargo add to install dependencies -- do not modify Cargo.toml directly.
Follow the 7 rules of great commit messages. No Claude Code attribution.
Do not use branches. Work directly out of main.
This is greenfield development. No backwards compatibility concerns. Refactor as needed.