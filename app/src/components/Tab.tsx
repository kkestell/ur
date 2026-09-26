import type { IDockviewPanelHeaderProps } from "dockview-react";
import { MessageSquare, Terminal, X } from "lucide-react";
import type { TabItem } from "../layout";
import { useWatch } from "../store/watch";
import { StatusMark } from "./StatusMark";

/**
 * A session or terminal tab: its icon, its title, its status mark when
 * relevant, and Close Tab. A terminal tab has no title until its
 * `terminal_changed` arrives.
 */
export function Tab({ api, params }: IDockviewPanelHeaderProps<TabItem>) {
  const watch = useWatch();
  let title;
  let mark = null;
  if (params.type === "session") {
    const summary = watch.sessions.find((session) => session.session === params.session);
    title = summary?.title ?? "New session";
    if (summary !== undefined && summary.status.type !== "idle") {
      mark = <StatusMark status={summary.status} />;
    }
  } else {
    title = watch.terminals.find((terminal) => terminal.terminal === params.terminal)?.title;
  }
  return (
    <div className="tab flex h-full max-w-96 items-center gap-2 px-1">
      {params.type === "session"
        ? <MessageSquare className="tab-kind shrink-0 text-fg-muted" size={12} strokeWidth={1.75} />
        : <Terminal className="tab-kind terminal-icon shrink-0 text-fg-muted" size={12} strokeWidth={1.75} />}
      <span className="label min-w-0 flex-1 truncate" title={title}>{title}</span>
      {mark}
      <button
        className="tab-close ml-1 flex size-4 shrink-0 items-center justify-center rounded text-fg-dim hover:bg-control hover:text-fg"
        title="Close Tab"
        // As dockview's own tab does, so dockview does not treat it as a
        // press on the tab.
        onPointerDown={(event) => event.preventDefault()}
        onClick={(event) => {
          event.preventDefault();
          api.close();
        }}
      >
        <X size={16} strokeWidth={1.75} />
      </button>
    </div>
  );
}
