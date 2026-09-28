# ur

ur is a tmux configuration for running coding agents in panes that persist across terminal
disconnects. Install tmux and Python 3, then run:

```sh
make install
ur
```

`make install` installs launchers and hooks for tmux marks in `~/.config/ur`, or
`$XDG_CONFIG_HOME/ur`, and installs the `ur` command to `~/.local/bin`. `ur` attaches to the `ur`
session on the `ur` tmux socket, starting it with this directory's `tmux.conf` if needed.
`ur restart`, run outside ur, stops the `ur` tmux server, ending the work in its panes, and starts a
fresh one. `make dev` runs ur on the separate `ur-dev` tmux socket. It replaces any existing
`ur-dev` server, attaches to a fresh `ur` session there, and reloads `tmux.conf` when it changes
while attached.

Run agents in some panes and ordinary shell commands in others. The prefix is Ctrl-b. Follow it with
`|` for a side split, `-` for a stacked split, `[` for copy mode, or `d` to detach. The mouse
selects and resizes panes. Standard window and copy bindings remain available. Each pane's border
shows its title. The status line lists each window, in its own color, by its name, or its directory
for an unnamed window. A yellow `?` after the name marks an agent waiting for permission, a green
`✓` a finished turn, and a red `✗` a turn error. A turn that ends while its pane is not focused
keeps its mark until the pane is focused. Run `ur` again to reattach.

To open one window per workspace when the tmux server starts, list the workspaces in
`~/.config/ur/tmux.conf`:

```
new-session -d -s ur -n ox -c ~/src/ox
set -w -t ur:ox @workspace on
new-window -d -t ur: -n ur -c ~/src/ur
set -w -t ur:ur @workspace on
```

Workspace windows stay open. Exiting their last shell starts a new one, and `&` and `x` refuse to
close them.

Detaching tmux leaves live work running. Stopping the tmux server ends live work in its panes.

An ox-tui pane gets the permission, completion, and error marks from its title.

Run `claude`, `codex`, or `opencode` in a pane to use your existing configuration, plugins, skills,
login, and history. Their usual homes (`~/.claude`, `~/.codex`, and `~/.config/opencode`, or your
own environment overrides) still apply. ur adds its hooks only to these launches; commands outside
ur keep their existing hooks. Panes start an interactive shell with ur's launchers first on `PATH`.
Aliases, shell functions, absolute executable paths, or shell startup files that replace `PATH` can
bypass the launchers.

A Claude Code pane gets the permission, completion, and error marks. This requires a Claude Code
version supporting `PostToolBatch` and `StopFailure` hooks. Approve ur's Codex hooks when Codex
first asks you to review them. A Codex pane gets the permission and completion marks. Codex has no
hook for a failed turn, so it gets no error mark.

An OpenCode pane gets the permission, completion, and error marks. A question from OpenCode or a
permission request from its subagents also marks the pane.

From this directory, run `tmux -L ur source-file "$PWD/tmux.conf"` to reload a changed `tmux.conf`
without detaching. Run `ur restart` after changing `XDG_CONFIG_HOME`.

Run `make e2e` for the isolated tmux tests.
