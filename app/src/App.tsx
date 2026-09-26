import { useEffect, useMemo, useState } from "react";
import { addWorkspace, newSession } from "./actions";
import { Editor } from "./components/Editor";
import { Sidebar } from "./components/Sidebar";
import { TerminalPane } from "./components/TerminalPane";
import { Thread } from "./components/Thread";
import {
  type Selection,
  connection,
  request,
  select,
  selection as loadSelection,
  setVisible,
} from "./ipc";
import type { PendingPermission } from "./ipc/bindings/PendingPermission";
import { shortcutKind } from "./keys";
import { useSession } from "./store/sessions";
import { apply as applyWatch, useWatch } from "./store/watch";
import { withPermissions } from "./transcript/permissions";

// One array for every render without pending requests, so nothing depending on
// `requests` re-runs for it.
const noRequests: PendingPermission[] = [];

export default function App() {
  const watch = useWatch();
  // `undefined` until the saved selection is read, so nothing is drawn that a
  // click could change before the saved selection replaces it.
  const [selection, setSelection] = useState<Selection | null>();

  useEffect(() => {
    connection()
      .then((connection) => applyWatch({ type: "connection", ...connection }))
      .catch(console.error);
    loadSelection().then(setSelection).catch(console.error);
    request({ type: "watch" }).catch(console.error);
  }, []);

  // The rendered selection, so the daemon is never asked to focus a session
  // the sidebar does not show.
  const visible =
    selection?.type === "session" &&
    watch.sessions.some((session) => session.session === selection.session)
      ? selection.session
      : null;
  useEffect(() => {
    setVisible(visible === null ? [] : [visible]).catch(console.error);
  }, [visible]);

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
  // daemon restarted without it, shows as no selection. So does a selected
  // terminal that has exited, or whose `terminal_changed` has not arrived.
  const terminal =
    selection?.type === "terminal"
      ? watch.terminals.find((terminal) => terminal.terminal === selection.terminal)
      : undefined;
  const selected =
    (selection?.type === "session" && visible === null) ||
    (selection?.type === "terminal" && terminal === undefined)
      ? null
      : selection;

  let main;
  if (terminal !== undefined) {
    // Keyed so switching terminals mounts a fresh view that attaches.
    main = <TerminalPane key={terminal.terminal} summary={terminal} />;
  } else if (selected?.type === "session") {
    // Keyed so the editor's draft and message stay with their session.
    main = <Session key={selected.session} id={selected.session} onSelect={onSelect} />;
  } else if (watch.workspaces.length === 0) {
    main = (
      <div className="empty">
        <div>No workspaces</div>
        <button className="button" onClick={() => void addWorkspace()}>
          Add Workspace
        </button>
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

function Session({ id, onSelect }: { id: string; onSelect: (selection: Selection) => void }) {
  const watch = useWatch();
  const thread = useSession(id);
  const summary = watch.sessions.find((session) => session.session === id);
  const status = summary?.status;
  const requests = status?.type === "needs_permission" ? status.requests : noRequests;
  // Memoized: `Session` renders on every watch event, and a fresh array each
  // time would scroll the thread to the bottom.
  const items = useMemo(() => withPermissions(thread?.blocks ?? [], requests), [thread, requests]);

  const answer = (request: PendingPermission, optionId: string) => {
    answerPermission(id, request.request_id, optionId);
  };

  // On `window`, so the shortcuts work with the editor focused. A shortcut
  // answers the oldest request with its first option of that kind, like
  // `ur approve` and `ur deny`.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const kind = shortcutKind(event);
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
  }, [id, requests]);

  return (
    <div className="session">
      <div className="session-header">
        <span className="label">{summary?.title ?? "New session"}</span>
        {summary !== undefined && (
          <button
            className="icon-button"
            title="New Session"
            onClick={() => void newSession(summary.workspace, onSelect)}
          >
            +
          </button>
        )}
      </div>
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
