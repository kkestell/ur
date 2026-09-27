import { showSessionMenu, showTerminalMenu, showWorkspaceMenu } from "../actions";
import { workspaceColorStyle } from "../colors";
import type { TabItem } from "../layout";
import {
  type WatchState,
  attentionCount,
  orderedWorkspaces,
  sessionServer,
  workspaceSessions,
  workspaceTerminals,
} from "../store/watch";
import { Plus, Terminal } from "lucide-react";
import { SessionIcon } from "../serverIcons";
import { AddWorkspaceButton } from "./AddWorkspaceButton";
import { StatusMark } from "./StatusMark";

export function Sidebar({
  watch,
  selection,
  onOpen,
  onConfigureServer,
}: {
  watch: WatchState;
  selection: TabItem | null;
  onOpen: (item: TabItem) => void;
  onConfigureServer: () => void;
}) {
  return (
    <nav className="sidebar min-w-0 flex-1 overflow-y-auto border-r-2 border-divider bg-panel pb-3 select-none">
      <div className="sidebar-header flex h-10 items-center gap-2 px-4 text-xs font-medium tracking-wide text-fg-dim uppercase">
        <span className="label min-w-0 flex-1">Workspaces</span>
        <button className="rounded px-1 hover:bg-control" title="Server settings" onClick={onConfigureServer}>Server</button>
        <AddWorkspaceButton className="icon-button flex size-7 items-center justify-center rounded text-fg-muted hover:bg-control hover:text-fg" title="Add Workspace">
          <Plus size={16} strokeWidth={1.75} />
        </AddWorkspaceButton>
      </div>
      {orderedWorkspaces(watch).map((workspace) => {
        const count = attentionCount(watch, workspace.name);
        return (
          <div key={workspace.name} className="workspace pb-2" style={workspaceColorStyle(workspace.color)}>
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
              {workspaceSessions(watch, workspace.name).map((session) => {
                const server = sessionServer(watch, session);
                const serverName = watch.servers.length > 1 ? (server?.name ?? "Unknown server") : null;
                const title = session.title ?? "New session";
                return <div
                  key={session.session}
                  className={
                    "row flex min-h-8 cursor-default items-center gap-2 rounded px-2 py-1.5 text-left" +
                    (selection?.type === "session" && selection.session === session.session
                      ? " selected text-fg-strong"
                      : "") +
                    (session.unread ? " unread font-semibold" : "")
                  }
                  title={serverName == null ? title : `${title} · ${serverName}`}
                  onClick={() => onOpen({ type: "session", session: session.session })}
                  onContextMenu={(event) => {
                    if (server?.capabilities?.sessionCapabilities?.delete != null) {
                      event.preventDefault();
                      void showSessionMenu(session);
                    }
                  }}
                >
                  <SessionIcon server={server} className="shrink-0 text-fg-dim" />
                  <span className="label min-w-0 flex-1 truncate">{title}</span>
                  {serverName != null && <span className="server max-w-24 shrink-0 truncate text-xs font-normal text-fg-dim">{serverName}</span>}
                  <StatusMark status={session.status} />
                </div>;
              })}
              {workspaceTerminals(watch, workspace.name).map((terminal) => (
                <div
                  key={terminal.terminal}
                  className={
                    "row flex min-h-8 cursor-default items-center gap-2 rounded px-2 py-1.5 text-left" +
                    (selection?.type === "terminal" && selection.terminal === terminal.terminal
                      ? " selected text-fg-strong"
                      : "")
                  }
                  title={terminal.title}
                  onClick={() => onOpen({ type: "terminal", terminal: terminal.terminal })}
                  onContextMenu={(event) => {
                    event.preventDefault();
                    void showTerminalMenu(terminal);
                  }}
                >
                  <Terminal className="terminal-icon shrink-0 text-fg-dim" size={12} strokeWidth={1.75} />
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
