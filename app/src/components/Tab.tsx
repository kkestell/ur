import type { IDockviewPanelHeaderProps } from "dockview-react";
import type { TabItem } from "../layout";
import { useWatch } from "../store/watch";
import { StatusMark } from "./StatusMark";

/**
 * The tab for a session or terminal: its kind, its title, and, for a session,
 * its status mark, which Close Tab replaces on hover. A terminal tab has no
 * title until its `terminal_changed` arrives.
 */
export function Tab({ api, params }: IDockviewPanelHeaderProps<TabItem>) {
  const watch = useWatch();
  let kind;
  let title;
  let mark;
  if (params.type === "session") {
    const summary = watch.sessions.find((session) => session.session === params.session);
    kind = "✦";
    title = summary?.title ?? "New session";
    mark = summary !== undefined && <StatusMark status={summary.status} />;
  } else {
    kind = ">_";
    title = watch.terminals.find((terminal) => terminal.terminal === params.terminal)?.title;
  }
  return (
    <div className="tab">
      <span className={params.type === "session" ? "tab-kind" : "tab-kind terminal-icon"}>
        {kind}
      </span>
      <span className="label">{title}</span>
      <span className="tab-end">
        {mark}
        <button
          className="tab-close"
          title="Close Tab"
          // As dockview's own tab does, so dockview does not treat it as a
          // press on the tab.
          onPointerDown={(event) => event.preventDefault()}
          onClick={(event) => {
            event.preventDefault();
            api.close();
          }}
        >
          ×
        </button>
      </span>
    </div>
  );
}
