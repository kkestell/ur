import {
  type DockviewApi,
  type DockviewReadyEvent,
  DockviewReact,
  type IDockviewHeaderActionsProps,
  type IDockviewPanelProps,
  type SerializedDockview,
  themeDark,
} from "dockview-react";
import "dockview-react/dist/styles/dockview.css";
import { Plus } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { addWorkspace, showNewMenu } from "../actions";
import { request, saveLayout, setVisible } from "../ipc";
import type { PendingPermission } from "../ipc/bindings/PendingPermission";
import { isMacPlatform, shortcutKind } from "../keys";
import { type TabItem, goneTabs, openTab, tabId, tabWorkspace, visibleSessions } from "../layout";
import { useSession } from "../store/sessions";
import { useWatch } from "../store/watch";
import { withPermissions } from "../transcript/permissions";
import { Editor } from "./Editor";
import { Tab } from "./Tab";
import { TerminalPane } from "./TerminalPane";
import { Thread } from "./Thread";

// One array for every render without pending requests, so nothing depending on
// `requests` re-runs for it.
const noRequests: PendingPermission[] = [];

const components = { session: SessionPanel, terminal: TerminalPanel };

/**
 * The panes and their tabs. Restores `saved`, less the tabs whose session or
 * terminal is gone, and saves the layout on every change. Render it only with
 * the watch snapshot, which the restore checks the tabs against.
 */
export function Layout({
  saved,
  onReady,
  onSelection,
  onTabClosed,
}: {
  saved: SerializedDockview | null;
  onReady: (api: DockviewApi | null) => void;
  onSelection: (selection: TabItem | null) => void;
  onTabClosed: (item: TabItem) => void;
}) {
  const watch = useWatch();
  const [api, setApi] = useState<DockviewApi>();
  const [visible, setVisibleSessions] = useState<string[]>([]);
  const previous = useRef(watch);
  const cleaning = useRef(new Set<string>());

  const ready = ({ api }: DockviewReadyEvent) => {
    if (saved !== null) {
      api.fromJSON(saved);
      closeTabs(api, goneTabs(tabItems(api), previous.current));
    }
    setApi(api);
  };

  // Registered after the restore, so a partial layout is not saved, and
  // disposed before dockview is torn down, so an empty one is not either.
  useEffect(() => {
    if (api === undefined) {
      return;
    }
    const update = () => {
      setVisibleSessions(visibleSessions(api));
      onSelection((api.activePanel?.params as TabItem | undefined) ?? null);
    };
    // Scrolls each pane's active tab fully into view once dockview has
    // rendered the change.
    let frame = 0;
    const revealActiveTabs = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        for (const tab of document.querySelectorAll<HTMLElement>(
          ".panes .dv-tabs-container .dv-tab.dv-active-tab",
        )) {
          tab.scrollIntoView({ block: "nearest", inline: "nearest" });
        }
      });
    };
    update();
    onReady(api);
    revealActiveTabs();
    const saving = api.onDidLayoutChange(() => {
      saveLayout(api.toJSON()).catch(console.error);
      update();
      revealActiveTabs();
    });
    const activating = api.onDidActivePanelChange(() => {
      update();
      revealActiveTabs();
    });
    const removing = api.onDidRemovePanel((panel) => {
      if (!cleaning.current.has(panel.id)) {
        onTabClosed(panel.params as TabItem);
      }
    });
    return () => {
      cancelAnimationFrame(frame);
      saving.dispose();
      activating.dispose();
      removing.dispose();
      onReady(null);
      onSelection(null);
    };
  }, [api]);

  // Dockview resizes panes on a sash press and moves tabs on a tab press
  // without stopping the press's default action, which starts a text
  // selection. A tab's `mousedown` is canceled rather than its `pointerdown`,
  // which dockview ignores once canceled.
  useEffect(() => {
    const onPointerDown = (event: PointerEvent) => {
      if ((event.target as Element).closest(".dv-sash") !== null) {
        event.preventDefault();
      }
    };
    const onMouseDown = (event: MouseEvent) => {
      if ((event.target as Element).closest(".dv-tab") !== null) {
        event.preventDefault();
      }
    };
    document.addEventListener("pointerdown", onPointerDown, true);
    document.addEventListener("mousedown", onMouseDown, true);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown, true);
      document.removeEventListener("mousedown", onMouseDown, true);
    };
  }, []);

  useEffect(() => {
    if (api !== undefined) {
      const gone = goneTabs(tabItems(api), watch, previous.current);
      for (const item of gone) {
        cleaning.current.add(tabId(item));
      }
      closeTabs(api, gone);
      cleaning.current.clear();
    }
    previous.current = watch;
  }, [api, watch]);

  // Only sessions in the watch state, so the daemon is never asked to focus a
  // session the sidebar does not show. Joined, so watch events that change
  // nothing do not resend it.
  const shown = JSON.stringify(
    visible.filter((id) => watch.sessions.some((session) => session.session === id)),
  );
  useEffect(() => {
    setVisible(JSON.parse(shown)).catch(console.error);
  }, [shown]);

  // Tabs move with pointer events. After a native HTML5 drag, WebKit sends no
  // `pointerdown` for the next press, and dockview activates tabs on
  // `pointerdown`.
  return (
    <DockviewReact
      className="panes min-h-0 min-w-0 flex-1 overflow-hidden"
      theme={themeDark}
      dndStrategy="pointer"
      components={components}
      defaultTabComponent={Tab}
      rightHeaderActionsComponent={PaneActions}
      watermarkComponent={Watermark}
      defaultRenderer="always"
      singleTabMode="default"
      disableFloatingGroups
      disableTabsOverflowList
      onReady={ready}
    />
  );
}

function tabItems(api: DockviewApi): TabItem[] {
  return api.panels.map((panel) => panel.params as TabItem);
}

function closeTabs(api: DockviewApi, items: TabItem[]) {
  for (const item of items) {
    api.removePanel(api.getPanel(tabId(item))!);
  }
}

/** The pane header's `+`, which opens the new menu in the active tab's workspace. */
function PaneActions({ activePanel, group, containerApi }: IDockviewHeaderActionsProps) {
  const watch = useWatch();
  const item = activePanel?.params as TabItem | undefined;
  const workspace = item === undefined ? undefined : tabWorkspace(watch, item);
  if (workspace === undefined) {
    return null;
  }
  return (
    <div className="pane-actions flex h-full items-center px-2">
      <button
        className="icon-button flex size-7 items-center justify-center rounded text-fg-muted hover:bg-control hover:text-fg"
        title="New"
        onClick={() => void showNewMenu(workspace, (item) => openTab(containerApi, item, group))}
      >
        <Plus size={16} strokeWidth={1.75} />
      </button>
    </div>
  );
}

/** The empty states, shown when no tab is open. */
function Watermark() {
  const watch = useWatch();
  if (watch.workspaces.length === 0) {
    return (
      <div className="empty flex h-full flex-col items-center justify-center gap-2 text-sm">
        <div>No workspaces</div>
        <button className="button rounded border border-control-edge bg-control px-3 py-1 hover:bg-control-hover" onClick={() => void addWorkspace()}>
          Add Workspace
        </button>
      </div>
    );
  }
  return <div className="empty hint flex h-full flex-col items-center justify-center text-center text-sm text-fg-dim">Select a session</div>;
}

function SessionPanel({ api, params }: IDockviewPanelProps<TabItem>) {
  const id = (params as Extract<TabItem, { type: "session" }>).session;
  const watch = useWatch();
  const thread = useSession(id);
  const status = watch.sessions.find((session) => session.session === id)?.status;
  const requests = status?.type === "needs_permission" ? status.requests : noRequests;
  // Memoized: `SessionPanel` renders on every watch event, and a fresh array
  // each time would scroll the thread to the bottom.
  const items = useMemo(() => withPermissions(thread?.blocks ?? [], requests), [thread, requests]);

  // Whether this is the selection, whose pending requests the permission
  // shortcuts answer.
  const [active, setActive] = useState(api.isActive);
  useEffect(() => {
    setActive(api.isActive);
    const listener = api.onDidActiveChange((event) => setActive(event.isActive));
    return () => listener.dispose();
  }, [api]);

  const answer = (request: PendingPermission, optionId: string) => {
    answerPermission(id, request.request_id, optionId);
  };

  // On `window`, so the shortcuts work with the editor focused. A shortcut
  // answers the oldest request with its first option of that kind, like
  // `ur approve` and `ur deny`.
  useEffect(() => {
    if (!active) {
      return;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      const kind = shortcutKind(event, isMacPlatform());
      const oldest = requests[0];
      if (kind === undefined || oldest === undefined) {
        return;
      }
      const option = oldest.request.options.find((option) => option.kind === kind);
      if (option !== undefined) {
        event.preventDefault();
        answer(oldest, option.optionId);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [id, requests, active]);

  return (
    <div className="session flex h-full min-h-0 flex-col">
      <Thread items={items} onAnswer={(request, option) => answer(request, option.optionId)} />
      <Editor
        session={id}
        status={status}
        thread={thread}
        capabilities={watch.capabilities}
      />
    </div>
  );
}

/** The terminal's view, once its `terminal_changed` has arrived. */
function TerminalPanel({ params }: IDockviewPanelProps<TabItem>) {
  const id = (params as Extract<TabItem, { type: "terminal" }>).terminal;
  const watch = useWatch();
  if (!watch.terminals.some((terminal) => terminal.terminal === id)) {
    return null;
  }
  return <TerminalPane terminal={id} />;
}

/**
 * Answers the request. An error, such as "permission request N is resolved"
 * when another daemon client answered first, is only logged: the daemon's
 * next `session_changed` removes the request from the thread anyway.
 */
function answerPermission(session: string, request_id: number, option_id: string) {
  request({ type: "answer_permission", session, request_id, option_id })
    .then((response) => {
      if (response.type === "error") {
        console.error(`answer_permission: ${response.message}`);
      }
    })
    .catch(console.error);
}
