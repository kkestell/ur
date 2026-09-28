# Full codebase and docs

## Scope and coverage

Reviewed the whole working tree as it stands, uncommitted, after the Rust ACP client was replaced by
the tmux configuration, launchers, and agent hooks: `tmux.conf`, `scripts/ur`, `scripts/install`,
`scripts/agent`, `scripts/dev`, `scripts/hooks/attention`, `scripts/hooks/claude.json`,
`scripts/hooks/codex.json`, `scripts/hooks/opencode.js`, `tests/test_ur.py`, `Makefile`,
`dprint.json`, `.gitignore`, and the docs `README.md`, `AGENTS.md`, `agents/architecture.md`,
`agents/code-style.md`, `agents/glossary.md`, `agents/testing.md`, `agents/todo.md`, and
`agents/issues.csv`. Historical plans, work logs, and reviews were read as context only. The newest
plan and work log describe the deleted Rust client; no plan or work log covers the current code.

Lenses: correctness, simplicity, testing, documentation, comments, resources, concurrency,
error-handling, and naming.

Coverage gaps: the Claude, Codex, and OpenCode hook event names and payload shapes were not checked
against those agents; the review takes the JSON files and the plugin at their word. The OpenCode
plugin was read but not run.

Behavior checked by experiment on isolated tmux sockets:

- Which program each of `claude`, `codex`, and `opencode` resolves to in a pane started from
  `tmux.conf` with the user's real home directory (OX-0018).
- Whether detaching a real terminal client clears `@ur-focused`, whether a turn that finishes while
  detached keeps its mark, and whether reattaching clears it. All three behave as `README.md` says.
- Whether tmux expands `${VAR}` in a config file at parse time. It does, outside single quotes.

## Fixed

- **No-op border style resets** (`tmux.conf:16`): `set -gu pane-border-style` and
  `set -gu pane-active-border-style` restore global options to defaults they already have on a fresh
  server, and nothing sets them before or after. Removed both lines and the assertion of the default
  in the pane border test.
- **Settings files written through a temporary file** (`scripts/install:27`): The agent settings
  JSON went through `mkstemp` and `os.replace`, while the launchers beside them were written
  directly. Nothing reads these files while install runs. They are now written directly, and
  `tempfile` is no longer imported.
- **Architecture says the scripts are Python and that marks belong to windows**
  (`agents/architecture.md:3`): `scripts/ur` is a shell script and the OpenCode hook is JavaScript,
  and marks are pane options rendered in the status line. The doc now says scripts and describes
  marking panes.
- **Testing doc describes a state directory** (`agents/testing.md:23`): No test creates a state
  directory since the Rust client was removed. The sentence now names the temporary home and tmux
  socket.
- **Issue statuses differ between `AGENTS.md` and the Ox skills** (`AGENTS.md:30`): The doc listed
  `unplanned` and `planned`; the skills and the newer rows use `unscheduled` and `scheduled`, so the
  log used both. The doc now lists the skills' statuses, and OX-0017 is `unscheduled`.
- **Open issues describe deleted code** (`agents/issues.csv:3`): OX-0002, 0003, 0004, 0006, 0009,
  0012, 0013, and 0015 concern the daemon, GUI, and fake server that no longer exist, so a plan to
  fix them would have nothing to change. They are now `wontfix`.

## Findings

### High

#### Correctness

- **OX-0018 Shell startup files put the real agents ahead of ur's launchers** (`tmux.conf:3`): The
  launchers rely on `default-command` prepending `~/.config/ur/bin` before the interactive shell
  starts, but the shell's own startup files run afterwards. The user's `.zshrc` prepends
  `~/.local/bin`, `~/.opencode/bin`, and mise's directories, so in a pane started from this config
  with the real home directory, `command -v` resolves `claude` to `~/.local/bin/claude`, `codex` to
  `/opt/homebrew/bin/codex`, and `opencode` to `~/.opencode/bin/opencode`, with ur's bin at position
  36 on `PATH`. No agent launched by name in ur gets its hooks, so no Claude, Codex, or OpenCode
  pane gets a mark. The install test cannot see this because it runs `/bin/sh` in a temporary home
  with no startup files, and `README.md:48` warns only about startup files that replace `PATH`. The
  fix needs a decision on how ur puts its directory first after the startup files run. Two options:
  start zsh with `ZDOTDIR` pointing at a ur-owned directory whose rc files source the user's and
  then prepend the launchers, or drop the `default-command` prepend and document one line for the
  end of `.zshrc` that prepends the launchers only when `$TMUX` names the `ur` socket. Making panes
  login shells would not help: `.zprofile` runs `brew shellenv`, which prepends `/opt/homebrew/bin`
  as well.

### Medium

#### Testing

- **OX-0019 Workspace windows have no test** (`tmux.conf:23`): `remain-on-exit` with the `pane-died`
  hook, the `&` and `x` bindings, and sourcing `~/.config/ur/tmux.conf` only on a fresh server that
  is not `ur-dev` are documented behavior with no test. The dev test checks only that the `ur-dev`
  socket skips the workspaces file, and the other tests use temporary homes without one. A
  regression could leave dead panes on screen, close a workspace window, or reopen every workspace
  on each config reload. Add a test that starts a server with a workspaces file in its temporary
  home, checks that the workspace window opens, exits its shell and checks that a new pane replaces
  it, exits a shell in an ordinary window and checks that the pane closes, and reloads the config
  and checks that no second copy of the workspace appears.

### Low

#### Correctness

- **OX-0020 The workspaces file ignores `XDG_CONFIG_HOME`** (`tmux.conf:27`): `scripts/install`
  writes launchers and settings under `$XDG_CONFIG_HOME/ur` when it is set, and `default-command`
  reads them from there, but the config sources the workspaces file from `~/.config/ur/tmux.conf`
  regardless, and `README.md:60` asks for `ur restart` after changing `XDG_CONFIG_HOME` as if
  everything followed it. With the variable set, ur reads two config directories. The user does not
  set it today. Either honor it in the config, since tmux expands `${XDG_CONFIG_HOME}` at parse time
  outside single quotes, or drop it everywhere and use `~/.config/ur` alone.

#### Simplicity

- **OX-0021 The permission watcher tracks Claude's process to stop early**
  (`scripts/hooks/attention:29`): `owner()` walks the process tree with `ps` to find Claude's pid,
  and `watch()` polls it so the watcher can exit if Claude dies without running its `SessionEnd`
  hook while a permission is pending. When that happens the watcher returns without clearing the
  mark, so the `?` stays until the next launch in the pane clears the request, which ends a watcher
  without this check anyway; a closed pane ends it too. About twenty lines and two subprocess calls
  per permission request handle a case the code does not otherwise recover from. Remove `owner()`,
  the `owner` field, and the `claude` parameter, or make the case worth handling by clearing the
  mark when Claude is gone.

#### Testing

- **OX-0022 The OpenCode plugin's turn and request tracking has no test**
  (`scripts/hooks/opencode.js:33`): The plugin decides which state to report from busy and idle
  transitions, aborted errors, subagent sessions, and outstanding request counts, and none of it
  runs in the tests. The install test checks only that the plugin's URI reaches OpenCode. The Claude
  transcript follower, by contrast, has a table of cases. A Node test that drives `handle` with
  recorded events and a fake `client.session.get`, asserting the sequence of states passed to the
  hook, would own this.

## Checks run

- `make check` — passed.
- `make e2e` — passed, 6 tests.

## Verdict

The tmux config, launchers, and hooks are small and read well, and the Claude transcript follower is
carefully tested. The one serious problem is OX-0018: on this machine the launchers never run,
because the shell's startup files push the real agents ahead of them, so the hook marks that the
README promises do not appear for any agent. That needs a decision on how the launchers win, and it
should come before more hook work. The rest is a missing test for workspace windows and three low
items.
