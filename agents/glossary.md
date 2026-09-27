# Glossary

## Naming

- Use server for the ACP agent process the ACP client launches. Reserve agent for SDK types and the
  agent side of ACP, such as agent messages.
- Session always means an ACP session. Qualify tmux session, tmux server, and tmux client.
- Use pane for a tmux pane.
- Use saved history for the server's durable copy of sessions.
- Say tool call title and ACP session config option when discussing those ACP fields.

## Terms

- **TUI**: The interactive terminal interface of the Rust ACP client.
- **Server**: An ACP agent process the ACP client runs. ACP calls it the agent.
- **Session**: One ACP conversation created by the client on its server connection.
- **Workspace**: The directory supplied when starting ur.
- **Turn**: One prompt, from sending it until the server finishes or fails it.
- **Turn error**: The error that ended a turn.
- **Saved history**: The server's durable copy of its sessions.
- **Ox**: The server ur is developed and tested against. It is not part of ur.
