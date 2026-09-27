import type { IDockviewPanelHeaderProps } from "dockview-react";
import { Terminal, X } from "lucide-react";
import { workspaceColorStyle } from "../colors";
import { type TabItem, tabColor, tabTitle } from "../layout";
import { sessionServer, useWatch } from "../store/watch";
import { SessionIcon } from "../serverIcons";
import { StatusMark } from "./StatusMark";

/**
 * A session or terminal tab: its server's icon or the terminal icon, its
 * title, its status mark when relevant, and Close Tab.
 */
export function Tab({ api, params }: IDockviewPanelHeaderProps<TabItem>) {
  const watch = useWatch();
  const title = tabTitle(watch, params);
  let tooltip;
  let mark = null;
  let icon;
  if (params.type === "session") {
    const summary = watch.sessions.find((session) => session.session === params.session);
    const server = sessionServer(watch, summary);
    tooltip = watch.servers.length > 1
      ? `${title} · ${server?.name ?? "Unknown server"}`
      : title;
    icon = <SessionIcon server={server} className="tab-kind shrink-0 text-fg-muted" />;
    if (summary !== undefined && summary.status.type !== "idle") {
      mark = <StatusMark status={summary.status} />;
    }
  } else {
    tooltip = title;
    icon = <Terminal className="tab-kind terminal-icon shrink-0 text-fg-muted" size={12} strokeWidth={1.75} />;
  }
  return (
    <div
      className="tab workspace-tab flex h-full max-w-96 items-center gap-2 px-2"
      style={workspaceColorStyle(tabColor(watch, params))}
      // Dockview activates a tab on `pointerdown`. After a native menu closes,
      // WebKit sends the next press only as mouse events, with no
      // `pointerdown`, so the tab also activates on `mousedown`.
      onMouseDown={(event) => {
        if (event.button === 0 && !api.isActive && (event.target as Element).closest(".tab-close") === null) {
          api.setActive();
        }
      }}
    >
      {icon}
      <span className="label min-w-0 flex-1 truncate" title={tooltip}>{title}</span>
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
