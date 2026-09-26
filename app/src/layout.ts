import type { DockviewApi, DockviewGroupPanel } from "dockview-react";
import type { WatchState } from "./store/watch";

/** The session or terminal one tab shows. It is the tab's panel params. */
export type TabItem = { type: "session"; session: string } | { type: "terminal"; terminal: number };

/** The tab's panel ID. Dockview rejects a second panel with the same ID. */
export function tabId(item: TabItem): string {
  return item.type === "session" ? `session:${item.session}` : `terminal:${item.terminal}`;
}

/**
 * Activates the item's tab, wherever it is, or opens one in `group`, else in
 * the active pane. With no panes, dockview makes one.
 */
export function openTab(api: DockviewApi, item: TabItem, group?: DockviewGroupPanel): void {
  const existing = api.getPanel(tabId(item));
  if (existing !== undefined) {
    existing.api.setActive();
    return;
  }
  api.addPanel({
    id: tabId(item),
    component: item.type,
    params: item,
    position: group === undefined ? undefined : { referenceGroup: group },
  });
}

/** The workspace of the item's session or terminal. */
export function tabWorkspace(watch: WatchState, item: TabItem): string | undefined {
  return item.type === "session"
    ? watch.sessions.find((session) => session.session === item.session)?.workspace
    : watch.terminals.find((terminal) => terminal.terminal === item.terminal)?.workspace;
}

/**
 * The tabs to close because their session or terminal is gone. Without
 * `previous`, when the layout is restored, that is every tab whose session or
 * terminal is not in `watch`. With it, only those that were in `previous`, so
 * a terminal tab opened before its `terminal_changed` arrives stays open.
 */
export function goneTabs(items: TabItem[], watch: WatchState, previous?: WatchState): TabItem[] {
  return items.filter(
    (item) => !contains(watch, item) && (previous === undefined || contains(previous, item)),
  );
}

/** The session of each pane's active tab. */
export function visibleSessions(api: DockviewApi): string[] {
  return api.groups.flatMap((group) => {
    const item = group.activePanel?.params as TabItem | undefined;
    return item?.type === "session" ? [item.session] : [];
  });
}

function contains(watch: WatchState, item: TabItem): boolean {
  return item.type === "session"
    ? watch.sessions.some((session) => session.session === item.session)
    : watch.terminals.some((terminal) => terminal.terminal === item.terminal);
}
