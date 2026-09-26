import { addWorkspace, showSessionMenu, showTerminalMenu, showWorkspaceMenu } from "../actions";
import type { TabItem } from "../layout";
import {
  type WatchState,
  attentionCount,
  orderedWorkspaces,
  workspaceSessions,
  workspaceTerminals,
} from "../store/watch";
import { MessageSquare, Plus, Terminal } from "lucide-react";
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
    <nav className="sidebar w-70 shrink-0 overflow-y-auto border-r border-divider bg-panel pb-3 select-none">
      <div className="sidebar-header flex h-10 items-center gap-2 px-4 text-xs font-medium tracking-wide text-fg-dim uppercase">
        <span className="label min-w-0 flex-1">Workspaces</span>
        <button className="icon-button flex size-7 items-center justify-center rounded text-fg-muted hover:bg-control hover:text-fg" title="Add Workspace" onClick={() => void addWorkspace()}>
          <Plus size={16} strokeWidth={1.75} />
        </button>
      </div>
      {orderedWorkspaces(watch).map((workspace) => {
        const count = attentionCount(watch, workspace.name);
        return (
          <div key={workspace.name} className="workspace mb-3">
            <div
              className="workspace-name flex min-h-8 items-center px-4 text-label font-semibold tracking-wide text-fg-dim uppercase"
              title={workspace.name}
              onContextMenu={(event) => {
                event.preventDefault();
                void showWorkspaceMenu(watch, workspace.name, onOpen);
              }}
            >
              <span className="label min-w-0 flex-1 truncate">{workspace.name}</span>
              {count > 0 && <span className="count ml-auto min-w-5 rounded-full bg-control-hover px-1.5 text-center text-label leading-5">{count}</span>}
            </div>
            <div className="workspace-sessions px-2">
              {workspaceSessions(watch, workspace.name).map((session) => (
                <div
                  key={session.session}
                  className={
                    "row flex min-h-8 cursor-default items-center gap-2 rounded px-2 py-1.5 text-left hover:bg-row-hover" +
                    (selection?.type === "session" && selection.session === session.session
                      ? " selected bg-row-selected text-fg-strong hover:bg-row-selected"
                      : "") +
                    (session.unread ? " unread font-semibold" : "")
                  }
                  title={session.title ?? "New session"}
                  onClick={() => onOpen({ type: "session", session: session.session })}
                  onContextMenu={(event) => {
                    if (canDelete) {
                      event.preventDefault();
                      void showSessionMenu(session);
                    }
                  }}
                >
                  <MessageSquare className="shrink-0 text-fg-dim" size={16} strokeWidth={1.75} />
                  <span className="label min-w-0 flex-1 truncate">{session.title ?? "New session"}</span>
                  <StatusMark status={session.status} />
                </div>
              ))}
              {workspaceTerminals(watch, workspace.name).map((terminal) => (
                <div
                  key={terminal.terminal}
                  className={
                    "row flex min-h-8 cursor-default items-center gap-2 rounded px-2 py-1.5 text-left hover:bg-row-hover" +
                    (selection?.type === "terminal" && selection.terminal === terminal.terminal
                      ? " selected bg-row-selected text-fg-strong hover:bg-row-selected"
                      : "")
                  }
                  title={terminal.title}
                  onClick={() => onOpen({ type: "terminal", terminal: terminal.terminal })}
                  onContextMenu={(event) => {
                    event.preventDefault();
                    void showTerminalMenu(terminal);
                  }}
                >
                  <Terminal className="terminal-icon shrink-0 text-fg-dim" size={16} strokeWidth={1.75} />
                  <span className="label min-w-0 flex-1 truncate">{terminal.title}</span>
                </div>
              ))}
            </div>
          </div>
        );
      })}
    </nav>
  );
}
