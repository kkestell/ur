import { expect, test, vi } from "vitest";
import type { SessionSummary } from "../ipc/bindings/SessionSummary";
import {
  attentionCount,
  initialWatch,
  orderedWorkspaces,
  reduceWatch,
  workspaceSessions,
} from "./watch";

// The store installs its Tauri listeners when it loads.
vi.mock("../ipc", () => ({
  onWatch: () => Promise.resolve(() => {}),
  onConnection: () => Promise.resolve(() => {}),
}));

function summary(
  session: string,
  workspace: string,
  updated_at: string | null,
  attention: Partial<Pick<SessionSummary, "status" | "unread">> = {},
): SessionSummary {
  return {
    session,
    workspace,
    status: { type: "idle", last_stop: null },
    unread: false,
    title: null,
    updated_at,
    ...attention,
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
    capabilities: null,
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

test("sessions_needing_attention_come_first", () => {
  const state = reduceWatch(connected, {
    type: "watch_snapshot",
    capabilities: null,
    workspaces: [{ name: "ws", path: "/ws" }],
    sessions: [
      summary("unread", "ws", "2026-01-01T00:00:00Z", { unread: true }),
      summary("read", "ws", "2026-04-01T00:00:00Z"),
      summary("failed", "ws", "2026-02-01T00:00:00Z", {
        status: { type: "failed", message: "boom" },
      }),
      summary("permission", "ws", "2026-03-01T00:00:00Z", {
        status: { type: "needs_permission", requests: [] },
      }),
    ],
  });
  expect(workspaceSessions(state, "ws").map((session) => session.session)).toEqual([
    "permission",
    "failed",
    "unread",
    "read",
  ]);
});

test("workspaces_needing_attention_come_first", () => {
  const state = reduceWatch(connected, {
    type: "watch_snapshot",
    capabilities: null,
    workspaces: [
      { name: "a", path: "/a" },
      { name: "b", path: "/b" },
      { name: "c", path: "/c" },
    ],
    sessions: [
      summary("s1", "a", null),
      summary("s2", "b", null),
      summary("s3", "c", null, { unread: true }),
      summary("s4", "c", null),
    ],
  });
  expect(orderedWorkspaces(state).map((workspace) => workspace.name)).toEqual(["c", "a", "b"]);
  expect(["a", "b", "c"].map((name) => attentionCount(state, name))).toEqual([0, 0, 1]);
});

test("a_removed_workspace_drops_its_sessions", () => {
  let state = reduceWatch(connected, {
    type: "watch_snapshot",
    capabilities: null,
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
    capabilities: null,
    workspaces: [{ name: "a", path: "/a" }],
    sessions: [summary("s1", "a", null)],
  });
  state = reduceWatch(state, { type: "connection", connected: false, socket: "/tmp/ur.sock" });
  expect(state).toEqual({ ...initialWatch, socket: "/tmp/ur.sock" });
});

test("a_deleted_session_leaves_the_sidebar", () => {
  let state = reduceWatch(connected, {
    type: "watch_snapshot",
    capabilities: null,
    workspaces: [{ name: "a", path: "/a" }],
    sessions: [summary("s1", "a", null), summary("s2", "a", null)],
  });
  state = reduceWatch(state, { type: "session_deleted", session: "s1" });
  expect(state.sessions.map((session) => session.session)).toEqual(["s2"]);
});

test("the_capabilities_come_from_the_snapshot_and_later_changes", () => {
  let state = reduceWatch(connected, {
    type: "watch_snapshot",
    capabilities: { loadSession: true },
    workspaces: [],
    sessions: [],
  });
  expect(state.capabilities).toEqual({ loadSession: true });
  state = reduceWatch(state, {
    type: "capabilities_changed",
    capabilities: { promptCapabilities: { image: true } },
  });
  expect(state.capabilities).toEqual({ promptCapabilities: { image: true } });
});
