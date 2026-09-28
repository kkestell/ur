# Architecture

ur is `tmux.conf` and the scripts beside it: scripts that install it and hooks that mark panes whose
agents need attention. The config is loaded explicitly on the `ur` socket and never replaces the
user's personal tmux config or server. ur neither builds nor links ox-tui; it reads ox-tui's
terminal title to mark its panes.

Agents other than ox-tui report through hooks. Panes put ur's launchers first on `PATH`, and each
launcher adds ur's hooks to one agent launch without changing the agent's own configuration. Every
hook calls one script that sets tmux pane options, and `tmux.conf` turns those options into marks in
the status line.
