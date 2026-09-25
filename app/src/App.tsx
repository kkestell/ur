import { useEffect, useState } from "react";
import { Editor } from "./components/Editor";
import { Sidebar } from "./components/Sidebar";
import { TerminalPane } from "./components/TerminalPane";
import { Thread } from "./components/Thread";
import { type Selection, connection, request, select, selection as loadSelection } from "./ipc";
import { useSession } from "./store/sessions";
import { apply as applyWatch, useWatch } from "./store/watch";

export default function App() {
  const watch = useWatch();
  // `undefined` until the saved selection is read, so nothing is drawn that a
  // click could change before the saved selection replaces it.
  const [selection, setSelection] = useState<Selection | null>();
  const [terminalError, setTerminalError] = useState<string>();

  useEffect(() => {
    connection()
      .then((connection) => applyWatch({ type: "connection", ...connection }))
      .catch(console.error);
    loadSelection().then(setSelection).catch(console.error);
    request({ type: "watch" }).catch(console.error);
  }, []);

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
  if (selection === undefined) {
    return null;
  }

  const onSelect = (selection: Selection) => {
    setSelection(selection);
    select(selection).catch(console.error);
  };

  // A selected session that is gone, because its workspace was removed or the
  // daemon restarted without it, shows as no selection.
  const selected =
    selection?.type === "session" &&
    !watch.sessions.some((session) => session.session === selection.session)
      ? null
      : selection;

  let main;
  if (selected?.type === "terminal") {
    main = terminalError !== undefined ? (
      <pre className="error">{terminalError}</pre>
    ) : (
      <TerminalPane onError={setTerminalError} />
    );
  } else if (selected?.type === "session") {
    // Keyed so the editor's draft and message stay with their session.
    main = <Session key={selected.session} id={selected.session} />;
  } else if (watch.workspaces.length === 0) {
    main = (
      <div className="empty">
        <div>No workspaces</div>
        <div className="hint">
          <code>ur workspace add &lt;name&gt; &lt;path&gt;</code>
        </div>
      </div>
    );
  } else {
    main = <div className="empty hint">Select a session</div>;
  }

  return (
    <div className="layout">
      <Sidebar watch={watch} selection={selected} onSelect={onSelect} />
      <div className="main">{main}</div>
    </div>
  );
}

function Session({ id }: { id: string }) {
  const watch = useWatch();
  const thread = useSession(id);
  const status = watch.sessions.find((session) => session.session === id)?.status;
  return (
    <div className="session">
      <Thread blocks={thread?.blocks ?? []} />
      <Editor session={id} status={status} />
    </div>
  );
}
