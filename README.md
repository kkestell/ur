# ur

ur is a small interactive ACP client. Each invocation starts one server and one new session in the
supplied directory. It requires an already authenticated server that supports text prompts without
client filesystem or terminal capabilities.

Build with Rust:

```sh
cargo build -p ur --release
./target/release/ur --server Ox /path/to/project
```

The directory defaults to the current directory. `scripts/run` builds a debug binary and forwards
its arguments to ur.

Configure servers in `$XDG_CONFIG_HOME/ur/config.json`, or `~/.config/ur/config.json`:

```json
{
  "servers": [
    { "name": "Ox", "command": "ox", "args": ["acp"] }
  ]
}
```

Use your server's actual executable and ACP arguments. Omit `--server` only when exactly one server
is configured. Replace any old config with this shape: IDs and icons are no longer accepted. There
is no migration. Old `state.json` and `gui.json` files are unused and may be deleted from
`$XDG_STATE_HOME/ur`, or `~/.local/state/ur`.

Type a prompt and press Enter. Paste preserves newlines and waits for Enter; the input line shows
newlines as `↵`. Long input shows its end. Backspace deletes the last character; Ctrl-U clears the
input. Slash commands are sent as ordinary text.

Permission requests show numbered choices. Enter a number and press Enter. Ctrl-C during a turn
cancels it and its pending permissions; ur waits for the turn to end before accepting another
prompt. When idle, Ctrl-C clears nonempty input or quits if empty. Ctrl-D quits immediately.

Replies remain plain text, including Markdown source, in terminal scrollback. ur has no local saved
history, session browser, or resume command. Any saved history belongs to the server.

For panes and persistence across terminal disconnects, install tmux and load the repository config
explicitly on a separate socket:

```sh
tmux -L ur -f /absolute/path/to/ur/tmux.conf new-session -A -s ur
```

Run ur in a pane and ordinary shell commands in others. The prefix is Ctrl-b. Follow it with `|` for
a side split, `-` for a stacked split, `[` for copy mode, or `d` to detach. The mouse selects and
resizes panes. Standard window and copy bindings remain available. Reattach with:

```sh
tmux -L ur attach-session -t ur
```

Detaching tmux leaves live work running. Quitting ur ends its server connection and child process;
stopping the tmux server ends live work in its panes. A failed or disconnected ACP server ends ur
with an error; start ur again to create a new session.

Run `make check` for Rust and documentation checks and `make e2e` for the isolated tmux tests.
