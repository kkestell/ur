import { expect, test, vi } from "vitest";
import { type TabItem, goneTabs } from "./layout";
import { type WatchState, initialWatch } from "./store/watch";

// The watch store installs its Tauri listeners when it loads.
vi.mock("./ipc", () => ({
  onWatch: () => Promise.resolve(() => {}),
  onConnection: () => Promise.resolve(() => {}),
}));

function watch(sessions: string[], terminals: number[]): WatchState {
  return {
    ...initialWatch,
    connected: true,
    hasSnapshot: true,
    workspaces: [{ name: "ws", path: "/ws" }],
    sessions: sessions.map((session) => ({
      session,
      workspace: "ws",
      status: { type: "idle", last_stop: null },
      unread: false,
      title: null,
      updated_at: null,
    })),
    terminals: terminals.map((terminal) => ({ terminal, workspace: "ws", title: "zsh" })),
  };
}

const session = (session: string): TabItem => ({ type: "session", session });
const terminal = (terminal: number): TabItem => ({ type: "terminal", terminal });

test.each<{
  name: string;
  items: TabItem[];
  watch: WatchState;
  previous?: WatchState;
  gone: TabItem[];
}>([
  {
    name: "restoring drops a missing session and a missing terminal",
    items: [session("kept"), session("missing"), terminal(1), terminal(2)],
    watch: watch(["kept"], [1]),
    gone: [session("missing"), terminal(2)],
  },
  {
    name: "a live change closes a deleted session and an exited terminal",
    items: [session("deleted"), session("kept"), terminal(1), terminal(2)],
    watch: watch(["kept"], [2]),
    previous: watch(["deleted", "kept"], [1, 2]),
    gone: [session("deleted"), terminal(1)],
  },
  {
    name: "a live change keeps a terminal whose terminal has not appeared yet",
    items: [terminal(1), terminal(2)],
    watch: watch([], [1]),
    previous: watch([], [1]),
    gone: [],
  },
])("tabs_close_when_their_session_or_terminal_is_gone: $name", ({ items, watch, previous, gone }) => {
  expect(goneTabs(items, watch, previous)).toEqual(gone);
});
