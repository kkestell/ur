# ur architecture

ur is an interactive Rust ACP client. Each process owns one server connection and one newly created
session in a workspace. tmux owns panes, shells, copying, scrollback, and persistence across
terminal disconnects. There is no daemon or desktop app.

## Boundaries

ACP is the contract. ur assigns no special meaning to server names, tools, models, or IDs. ACP SDK
types cross the connection boundary unchanged. The client advertises no optional capabilities;
servers must already be authenticated and support text prompts without client filesystem or terminal
services.

The TUI owns terminal output and input. Notifications and permission handlers enqueue events without
waiting for input. One event loop prints output, edits a single input line, and answers the oldest
permission request. Only one prompt can run at a time. Cancellation answers pending and newly
arriving permissions as cancelled until the prompt response arrives.

## Ownership

- Each ur process launches and owns its server process. Quitting closes the connection and ends the
  child process. Server failure ends ur; there is no restart supervisor.
- The config file holds named executable commands and arguments. ur only reads it and maintains no
  workspace or layout state.
- Saved history belongs to the ACP server. ur neither stores transcripts nor resumes sessions.
- Terminal scrollback belongs to the terminal or tmux. ur prints on the normal screen without a
  retained screen model. Partial tool updates retain omitted fields by tool call ID.
- tmux remains independent of ur. Its config is loaded explicitly on a separate socket. Rust code
  neither controls tmux nor polls ACP status for it.

## Runtime

Tokio runs the ACP connection and prompt tasks. Crossterm's async event stream supplies keyboard,
paste, and resize events. Server diagnostics pass through the terminal output owner. Terminal mode
is restored on ordinary exit, errors, and panic.
