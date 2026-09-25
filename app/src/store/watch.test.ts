import { expect, test, vi } from "vitest";
import type { SessionSummary } from "../ipc/bindings/SessionSummary";
import { initialWatch, reduceWatch, workspaceSessions } from "./watch";

// The store installs its Tauri listeners when it loads.
vi.mock("../ipc", () => ({
  onWatch: () => Promise.resolve(() => {}),
  onConnection: () => Promise.resolve(() => {}),
}));

function summary(session: string, workspace: string, updated_at: string | null): SessionSummary {
  return {
    session,
    workspace,
    status: { type: "idle", last_stop: null },
    unread: false,
    title: null,
    updated_at,
  };
}

const connected = reduceWatch(initialWatch, {
  type: "connection",
  connected: true,
  socket: "/tmp/ur.sock",
});

test("sessions_order_by_last_activity", () => {
  const state = reduceWatch(connected, {
    type: "watch_snapshot",
    workspaces: [{ name: "ws", path: "/ws" }],
    sessions: [
      summary("old", "ws", "2026-01-01T00:00:00Z"),
      summary("new", "ws", "2026-02-01T00:00:00Z"),
      summary("fresh", "ws", null),
      summary("other", "other", "2026-03-01T00:00:00Z"),
    ],
  });
  expect(workspaceSessions(state, "ws").map((session) => session.session)).toEqual([
    "fresh",
    "new",
    "old",
  ]);
});

test("a_removed_workspace_drops_its_sessions", () => {
  let state = reduceWatch(connected, {
    type: "watch_snapshot",
    workspaces: [
      { name: "a", path: "/a" },
      { name: "b", path: "/b" },
    ],
    sessions: [summary("s1", "a", null), summary("s2", "b", null)],
  });
  state = reduceWatch(state, { type: "workspace_removed", name: "a" });
  expect(state.workspaces.map((workspace) => workspace.name)).toEqual(["b"]);
  expect(state.sessions.map((session) => session.session)).toEqual(["s2"]);
});

test("a_disconnect_clears_the_watch_state", () => {
  let state = reduceWatch(connected, {
    type: "watch_snapshot",
    workspaces: [{ name: "a", path: "/a" }],
    sessions: [summary("s1", "a", null)],
  });
  state = reduceWatch(state, { type: "connection", connected: false, socket: "/tmp/ur.sock" });
  expect(state).toEqual({ ...initialWatch, socket: "/tmp/ur.sock" });
});
