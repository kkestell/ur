# TODO

- [x] [Minimal TUI and tmux](plans/2026-09-27-001-minimal-tui-and-tmux.md): one ur process per
      server and session, with tmux supplying panes and persistence.

Acceptance check: run ur with a configured, authenticated server in one tmux pane beside a shell.
Send a prompt, answer a permission request, and cancel a running turn. Detach while waiting for
permission, reattach, answer it, and send another prompt. Earlier output remains in scrollback and
the adjacent shell remains usable. Quitting ur restores the terminal and ends its server process.
