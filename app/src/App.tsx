import type { DockviewApi, SerializedDockview } from "dockview-react";
import { useEffect, useRef, useState } from "react";
import { Layout } from "./components/Layout";
import { Sidebar } from "./components/Sidebar";
import { SidebarHandle, defaultSidebarWidth } from "./components/SidebarHandle";
import { ServerSetup } from "./components/ServerSetup";
import { connection, layout as loadLayout, request, saveSidebarWidth, sidebarWidth } from "./ipc";
import { newSession, newTerminal } from "./actions";
import { isMacPlatform, newShortcut, tabShortcut } from "./keys";
import { type TabItem, openTab, tabId, tabWorkspace } from "./layout";
import { apply as applyWatch, orderedWorkspaces, useWatch } from "./store/watch";

export default function App() {
  const watch = useWatch();
  // `undefined` until the saved layout is read, so nothing is drawn that a
  // click could change before the saved layout replaces it. Read on each
  // connect, since `Layout` restores it on each connect.
  const [saved, setSaved] = useState<SerializedDockview | null>();
  const [api, setApi] = useState<DockviewApi | null>(null);
  const [selection, setSelection] = useState<TabItem | null>(null);
  const recentlyClosed = useRef<TabItem[]>([]);
  // `undefined` until the saved sidebar width is read.
  const [width, setWidth] = useState<number>();
  const [editingServer, setEditingServer] = useState(false);
  const showSetup = watch.hasSnapshot && (watch.serverState.command === null || editingServer);

  useEffect(() => {
    if (watch.hasSnapshot && watch.serverState.command === null) setEditingServer(true);
  }, [watch.hasSnapshot, watch.serverState.command]);

  useEffect(() => {
    sidebarWidth()
      .then((width) => setWidth(width ?? defaultSidebarWidth))
      .catch((error) => {
        console.error(error);
        setWidth(defaultSidebarWidth);
      });
    connection()
      .then((connection) => applyWatch({ type: "connection", ...connection }))
      .catch(console.error);
    request({ type: "watch" }).catch(console.error);
  }, []);

  useEffect(() => {
    if (watch.connected) {
      loadLayout().then(setSaved).catch(console.error);
    } else {
      setSaved(undefined);
    }
  }, [watch.connected]);

  // Capture shortcuts before the editor or terminal handles the key.
  useEffect(() => {
    if (api === null || showSetup) {
      return;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      const mac = isMacPlatform();
      const opens = newShortcut(event, mac);
      const action = tabShortcut(event, mac);
      if (opens === undefined && action === undefined) {
        return;
      }
      event.preventDefault();
      event.stopPropagation();
      if (event.repeat) {
        return;
      }
      if (action === "close") {
        if (api.activePanel !== undefined) {
          api.removePanel(api.activePanel);
        }
        return;
      }
      if (action === "reopen") {
        while (recentlyClosed.current.length > 0) {
          const item = recentlyClosed.current.pop()!;
          if (tabWorkspace(watch, item) !== undefined && api.getPanel(tabId(item)) === undefined) {
            openTab(api, item);
            break;
          }
        }
        return;
      }
      const workspace =
        (selection === null ? undefined : tabWorkspace(watch, selection)) ??
        orderedWorkspaces(watch)[0]?.name;
      if (workspace === undefined) {
        return;
      }
      const onOpen = (item: TabItem) => openTab(api, item);
      void (opens === "session" ? newSession(workspace, onOpen) : newTerminal(workspace, onOpen));
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [api, selection, watch, showSetup]);

  if (!watch.connected) {
    return (
      <div className="empty connecting flex h-full flex-col items-center justify-center gap-2 text-sm">
        <div className="spinner size-4 animate-spin rounded-full border-2 border-fg-dim border-t-transparent" />
        <div>Connecting to {watch.socket}</div>
        {watch.connectionError && <div className="hint text-center text-red-400">{watch.connectionError}</div>}
      </div>
    );
  }
  if (saved === undefined || width === undefined) {
    return null;
  }

  const onOpen = (item: TabItem) => {
    if (api !== null) {
      openTab(api, item);
    }
  };

  // `Layout` restores the saved layout against the watch snapshot, and a
  // disconnect unmounts it, so reconnecting restores it against the new one.
  return (
    <div className="relative h-full">
      <div className="layout flex h-full min-w-0 overflow-hidden border-t border-divider">
        <div className="relative flex shrink-0" style={{ width }}>
          <Sidebar watch={watch} selection={selection} onOpen={onOpen} onConfigureServer={() => setEditingServer(true)} />
          <SidebarHandle
            width={width}
            onResize={setWidth}
            onResizeEnd={(width) => {
              setWidth(width);
              saveSidebarWidth(width).catch(console.error);
            }}
          />
        </div>
        <div className="main flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
          {!watch.serverState.connected && watch.serverState.error && (
            <button className="server-banner bg-control px-3 py-2 text-left text-sm" onClick={() => setEditingServer(true)}>
              Server: {watch.serverState.error} · Change server
            </button>
          )}
          {watch.hasSnapshot && (
            <Layout
              saved={saved}
              onReady={setApi}
              onSelection={setSelection}
              onTabClosed={(item) => recentlyClosed.current.push(item)}
            />
          )}
        </div>
      </div>
      {showSetup && (
        <div className="absolute inset-0 z-50 bg-panel">
          <ServerSetup state={watch.serverState} onSaved={() => setEditingServer(false)} onCancel={watch.serverState.command === null ? undefined : () => setEditingServer(false)} />
        </div>
      )}
    </div>
  );
}
