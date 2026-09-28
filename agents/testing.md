# Testing

`make check` checks the Markdown formatting. `make e2e` runs the tmux tests. Tests need no model
provider or agent.

## Where tests go

The tmux config, install scripts, launchers, and hooks belong in `tests`, as Python `unittest` tests
against `tmux.conf` on an isolated tmux socket. They never start ox-tui.

## Test discipline

Each test owns one durable, observable guarantee. Add tests at the closest stable boundary and
rewrite or remove tests when their guarantees change. Test the same assertion at different layers
only when those layers have distinct failure modes. Use table-driven cases for varied inputs to the
same behavior. Keep fixtures proportional to the guarantee and test behavior rather than internals.

Before finishing changes to tests, inspect the complete test diff and report which guarantees
gained, lost, or moved their owning tests.

## Terminal integration

Each test gets a temporary home and tmux socket. Use bounded waits and terminate only that test's
tmux server. Failures include the captured pane text. These tests require tmux on PATH and do not
use the user's personal tmux config or server.
