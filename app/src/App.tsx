import type { DockviewApi, SerializedDockview } from "dockview-react";
import { useEffect, useState } from "react";
import { Layout } from "./components/Layout";
import { Sidebar } from "./components/Sidebar";
import { connection, layout as loadLayout, request } from "./ipc";
import { type TabItem, openTab } from "./layout";
import { apply as applyWatch, useWatch } from "./store/watch";

export default function App() {
  const watch = useWatch();
  // `undefined` until the saved layout is read, so nothing is drawn that a
  // click could change before the saved layout replaces it. Read on each
  // connect, since `Layout` restores it on each connect.
  const [saved, setSaved] = useState<SerializedDockview | null>();
  const [api, setApi] = useState<DockviewApi | null>(null);
  const [selection, setSelection] = useState<TabItem | null>(null);

  useEffect(() => {
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

  if (!watch.connected) {
    return (
      <div className="empty connecting">
        <div className="spinner" />
        <div>Connecting to {watch.socket}</div>
        <div className="hint">
          Start the daemon with <code>ur daemon</code>
        </div>
      </div>
    );
  }
  if (saved === undefined) {
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
    <div className="layout">
      <Sidebar watch={watch} selection={selection} onOpen={onOpen} />
      <div className="main">
        {watch.hasSnapshot && (
          <Layout saved={saved} onReady={setApi} onSelection={setSelection} />
        )}
      </div>
    </div>
  );
}
