import { addWorkspace, showSessionMenu, showTerminalMenu, showWorkspaceMenu } from "../actions";
import type { TabItem } from "../layout";
import {
  type WatchState,
  attentionCount,
  orderedWorkspaces,
  workspaceSessions,
  workspaceTerminals,
} from "../store/watch";
import { StatusMark } from "./StatusMark";

export function Sidebar({
  watch,
  selection,
  onOpen,
}: {
  watch: WatchState;
  selection: TabItem | null;
  onOpen: (item: TabItem) => void;
}) {
  const canDelete = watch.capabilities?.sessionCapabilities?.delete != null;
  return (
    <nav className="sidebar">
      <div className="sidebar-header">
        <span className="label">Workspaces</span>
        <button className="icon-button" title="Add Workspace" onClick={() => void addWorkspace()}>
          +
        </button>
      </div>
      {orderedWorkspaces(watch).map((workspace) => {
        const count = attentionCount(watch, workspace.name);
        return (
          <div key={workspace.name} className="workspace">
            <div
              className="workspace-name"
              onContextMenu={(event) => {
                event.preventDefault();
                void showWorkspaceMenu(watch, workspace.name, onOpen);
              }}
            >
              <span className="label">{workspace.name}</span>
              {count > 0 && <span className="count">{count}</span>}
            </div>
            <div className="workspace-sessions">
              {workspaceSessions(watch, workspace.name).map((session) => (
                <div
                  key={session.session}
                  className={
                    "row" +
                    (selection?.type === "session" && selection.session === session.session
                      ? " selected"
                      : "") +
                    (session.unread ? " unread" : "")
                  }
                  onClick={() => onOpen({ type: "session", session: session.session })}
                  onContextMenu={(event) => {
                    if (canDelete) {
                      event.preventDefault();
                      void showSessionMenu(session);
                    }
                  }}
                >
                  <span className="label">{session.title ?? "New session"}</span>
                  <StatusMark status={session.status} />
                </div>
              ))}
              {workspaceTerminals(watch, workspace.name).map((terminal) => (
                <div
                  key={terminal.terminal}
                  className={
                    "row" +
                    (selection?.type === "terminal" && selection.terminal === terminal.terminal
                      ? " selected"
                      : "")
                  }
                  onClick={() => onOpen({ type: "terminal", terminal: terminal.terminal })}
                  onContextMenu={(event) => {
                    event.preventDefault();
                    void showTerminalMenu(terminal);
                  }}
                >
                  <span className="terminal-icon">&gt;_</span>
                  <span className="label">{terminal.title}</span>
                </div>
              ))}
            </div>
          </div>
        );
      })}
    </nav>
  );
}
