# CLI Integration Tests

Each file describes a sequence of `ur` commands and their expected
outcomes.

## Setup

```bash
make install          # builds release binary + system extensions
                      # installs to ~/.local/bin/ur and ~/.ur/extensions/system/
```

Create a fresh workspace for the test session:

```bash
export W=$(mktemp -d)
```

All commands below use `ur -w "$W"` to isolate state.

### API keys

Provider tests need keys from `.env` at the repo root:

```bash
set -a && source .env && set +a
```

If `.env` is missing or a key is absent, add it there.

### Cleanup

```bash
rm -rf "$W"
```

## Tests

- [extensions.md](extensions.md) — Discovery, enable/disable, slot constraints
- [model-roles.md](model-roles.md) — Role assignment, listing, validation
- [model-settings.md](model-settings.md) — Extension config set/get/list, constraint validation
- [agent-turn.md](agent-turn.md) — Deterministic agent turn with test/echo LLM
- [google-provider.md](google-provider.md) — Live Google Gemini completions (needs `GOOGLE_API_KEY`)
- [openrouter-provider.md](openrouter-provider.md) — Live OpenRouter completions (needs `OPENROUTER_API_KEY`)
