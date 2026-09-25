import type { Selection } from "../ipc";
import { type WatchState, workspaceSessions } from "../store/watch";

export function Sidebar({
  watch,
  selection,
  onSelect,
}: {
  watch: WatchState;
  selection: Selection | null;
  onSelect: (selection: Selection) => void;
}) {
  return (
    <nav className="sidebar">
      <div className="sidebar-header">Workspaces</div>
      {watch.workspaces.map((workspace) => (
        <div key={workspace.name} className="workspace">
          <div className="workspace-name">{workspace.name}</div>
          <div className="workspace-sessions">
            {workspaceSessions(watch, workspace.name).map((session) => (
              <div
                key={session.session}
                className={
                  "row" +
                  (selection?.type === "session" && selection.session === session.session
                    ? " selected"
                    : "")
                }
                onClick={() => onSelect({ type: "session", session: session.session })}
              >
                {session.title ?? "New session"}
              </div>
            ))}
          </div>
        </div>
      ))}
      <div
        className={"row terminal-row" + (selection?.type === "terminal" ? " selected" : "")}
        onClick={() => onSelect({ type: "terminal" })}
      >
        Terminal
      </div>
    </nav>
  );
}
