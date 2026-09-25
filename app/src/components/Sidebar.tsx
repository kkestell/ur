import type { Selection } from "../ipc";
import type { SessionSummary } from "../ipc/bindings/SessionSummary";
import { type WatchState, attentionCount, orderedWorkspaces, workspaceSessions } from "../store/watch";

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
      {orderedWorkspaces(watch).map((workspace) => {
        const count = attentionCount(watch, workspace.name);
        return (
          <div key={workspace.name} className="workspace">
            <div className="workspace-name">
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
                  onClick={() => onSelect({ type: "session", session: session.session })}
                >
                  <span className="label">{session.title ?? "New session"}</span>
                  <StatusMark status={session.status} />
                </div>
              ))}
            </div>
          </div>
        );
      })}
      <div
        className={"row terminal-row" + (selection?.type === "terminal" ? " selected" : "")}
        onClick={() => onSelect({ type: "terminal" })}
      >
        Terminal
      </div>
    </nav>
  );
}

/** The mark at the right edge of a session row: nothing when idle. */
function StatusMark({ status }: { status: SessionSummary["status"] }) {
  switch (status.type) {
    case "idle":
      return null;
    case "working":
      return <span className="status spinner" />;
    case "needs_permission":
      return <span className="status dot" />;
    case "failed":
      return <span className="status failed">!</span>;
  }
}
