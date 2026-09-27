# Glossary

## Naming

- Use server for the ACP agent process the daemon launches. Reserve agent for SDK type names, test
  agents, and the agent side of the UI (agent session, agent message) where it contrasts with
  terminals.
- Qualify client as ACP client or daemon client whenever the surrounding text does not make it
  obvious.
- Qualify connection as ACP connection or socket connection whenever the surrounding text does not
  make it obvious.
- Qualify protocol as ACP or wire protocol. Never write ACP protocol.
- Session always means an ACP session. Say terminal, never terminal session.
- Use transcript for the daemon's copy of a session's content and thread for the GUI's view of it.
  History appears only as saved history, the server's durable copy.
- Say session title, terminal title, or tool call title. Never write an unqualified title.
- Qualify attachment as terminal attachment or image attachment.
- Attach refers only to terminals. Sessions are watched or subscribed.
- Say config option only for an ACP session config option.

## Terms

- **Server**: An ACP agent process the daemon runs. ACP calls it the agent.
- **Daemon client**: Anything connected to the daemon's socket, such as the GUI.
- **Core**: The GUI's Rust side, and its only daemon client.
- **Webview**: The GUI's frontend. It reaches the daemon only through the core.
- **Workspace**: A named directory that sessions and terminals belong to.
- **Session key**: The daemon's identifier for a session: its server and its ACP session ID.
- **Saved history**: The server's durable copy of its sessions.
- **Load**: Rebuilding a session's transcript from saved history.
- **Transcript**: The daemon's in-memory record of one session's content.
- **Thread**: The GUI's view of one session's transcript.
- **Turn**: One prompt, from sending it until the server finishes or fails it.
- **Turn error**: The error that ended a turn, kept in the transcript.
- **Unread**: A session whose turn ended while no daemon client was showing it.
- **Needs attention**: A session waiting for permission, failed, or unread.
- **Focus**: The sessions a daemon client is showing.
- **Busy**: The answer to a request that conflicts with an operation already running on the session.
- **Terminal attachment**: A daemon client receiving a terminal's current screen and live output.
- **Ox**: The server ur is developed and tested against. It is not part of ur.
