import { expect, test, vi } from "vitest";
import type { SessionSummary } from "../ipc/bindings/SessionSummary";
import type { TerminalSummary } from "../ipc/bindings/TerminalSummary";
import {
  attentionCount,
  initialWatch,
  orderedWorkspaces,
  reduceWatch,
  workspaceSessions,
  workspaceTerminals,
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
    server: "one",
    workspace,
    status: { type: "idle", last_stop: null },
    unread: false,
    title: null,
    updated_at,
    ...attention,
  };
}

function terminal(id: number, workspace: string, title = "zsh"): TerminalSummary {
  return { terminal: id, workspace, title };
}

const connected = reduceWatch(initialWatch, {
  type: "connection",
  connected: true,
  socket: "/tmp/ur.sock",
  error: null,
});

test("sessions_stay_in_creation_order_newest_first", () => {
  const state = reduceWatch(connected, {
    type: "watch_snapshot",
    terminals: [],
    servers: [],
    config_error: null,
    workspaces: [{ name: "ws", path: "/ws", color: "blue" }],
    sessions: [
      summary("old", "ws", "2026-03-01T00:00:00Z"),
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

test("session_activity_and_attention_do_not_change_order", () => {
  let state = reduceWatch(connected, {
    type: "watch_snapshot",
    terminals: [],
    servers: [],
    config_error: null,
    workspaces: [{ name: "ws", path: "/ws", color: "blue" }],
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
  const ids = () => workspaceSessions(state, "ws").map((session) => session.session);
  expect(ids()).toEqual(["permission", "failed", "read", "unread"]);
  state = reduceWatch(state, {
    type: "session_changed",
    summary: summary("unread", "ws", "2026-05-01T00:00:00Z", { unread: true }),
  });
  expect(ids()).toEqual(["permission", "failed", "read", "unread"]);
});

test("workspaces_are_alphabetical_even_with_attention", () => {
  const state = reduceWatch(connected, {
    type: "watch_snapshot",
    terminals: [],
    servers: [],
    config_error: null,
    workspaces: [
      { name: "c", path: "/c", color: "blue" },
      { name: "b", path: "/b", color: "blue" },
      { name: "a", path: "/a", color: "blue" },
    ],
    sessions: [
      summary("s1", "a", null),
      summary("s2", "b", null),
      summary("s3", "c", null, { unread: true }),
      summary("s4", "c", null),
    ],
  });
  expect(orderedWorkspaces(state).map((workspace) => workspace.name)).toEqual(["a", "b", "c"]);
  expect(["a", "b", "c"].map((name) => attentionCount(state, name))).toEqual([0, 0, 1]);
});

test("a_removed_workspace_drops_its_sessions", () => {
  let state = reduceWatch(connected, {
    type: "watch_snapshot",
    terminals: [],
    servers: [],
    config_error: null,
    workspaces: [
      { name: "a", path: "/a", color: "blue" },
      { name: "b", path: "/b", color: "blue" },
    ],
    sessions: [summary("s1", "a", null), summary("s2", "b", null)],
  });
  state = reduceWatch(state, { type: "workspace_removed", name: "a" });
  expect(state.workspaces.map((workspace) => workspace.name)).toEqual(["b"]);
  expect(state.sessions.map((session) => session.session)).toEqual(["s2"]);
});

test("a_changed_workspace_replaces_the_one_with_its_name", () => {
  let state = reduceWatch(connected, {
    type: "watch_snapshot",
    terminals: [],
    servers: [],
    config_error: null,
    workspaces: [{ name: "a", path: "/a", color: "blue" }],
    sessions: [],
  });
  state = reduceWatch(state, {
    type: "workspace_changed",
    workspace: { name: "a", path: "/a", color: "peach" },
  });
  state = reduceWatch(state, {
    type: "workspace_changed",
    workspace: { name: "b", path: "/b", color: "green" },
  });
  expect(state.workspaces).toEqual([
    { name: "a", path: "/a", color: "peach" },
    { name: "b", path: "/b", color: "green" },
  ]);
});

test("a_disconnect_clears_the_watch_state", () => {
  let state = reduceWatch(connected, {
    type: "watch_snapshot",
    terminals: [terminal(1, "a")],
    servers: [],
    config_error: null,
    workspaces: [{ name: "a", path: "/a", color: "blue" }],
    sessions: [summary("s1", "a", null)],
  });
  state = reduceWatch(state, { type: "connection", connected: false, socket: "/tmp/ur.sock", error: null });
  expect(state).toEqual({ ...initialWatch, socket: "/tmp/ur.sock" });
});

test("a_deleted_session_leaves_the_sidebar", () => {
  let state = reduceWatch(connected, {
    type: "watch_snapshot",
    terminals: [],
    servers: [],
    config_error: null,
    workspaces: [{ name: "a", path: "/a", color: "blue" }],
    sessions: [summary("s1", "a", null), summary("s2", "a", null)],
  });
  state = reduceWatch(state, { type: "session_deleted", session: "s1" });
  expect(state.sessions.map((session) => session.session)).toEqual(["s2"]);
});

test("server_changes_replace_the_complete_list_and_clear_capabilities", () => {
  const one = { id: "one", name: "One", icon: "claude" as const, command: "/bin/one", args: [], connected: true, error: null, capabilities: { loadSession: true } };
  const two = { id: "two", name: "Two", icon: "claude" as const, command: "/bin/two", args: [], connected: true, error: null, capabilities: { loadSession: true } };
  let state = reduceWatch(connected, { type: "watch_snapshot", terminals: [], servers: [one, two], config_error: null, workspaces: [], sessions: [] });
  expect(state.servers.map((server) => server.id)).toEqual(["one", "two"]);
  state = reduceWatch(state, { type: "servers_changed", servers: [one, { ...two, connected: false, capabilities: null, error: "failed" }], config_error: null });
  expect(state.servers[0].capabilities).toEqual({ loadSession: true });
  expect(state.servers[1].capabilities).toBeNull();
  expect(state.servers[1].error).toBe("failed");
});

test("config_error_recovers_after_adding_a_server", () => {
  let state = reduceWatch(connected, { type: "watch_snapshot", terminals: [], servers: [], config_error: "bad config", workspaces: [], sessions: [] });
  expect(state.configError).toBe("bad config");
  state = reduceWatch(state, { type: "servers_changed", servers: [], config_error: null });
  expect(state.configError).toBeNull();
});

test("terminals_follow_watch_events", () => {
  let state = reduceWatch(connected, {
    type: "watch_snapshot",
    terminals: [terminal(1, "a")],
    servers: [],
    config_error: null,
    workspaces: [
      { name: "a", path: "/a", color: "blue" },
      { name: "b", path: "/b", color: "blue" },
    ],
    sessions: [],
  });
  expect(state.terminals).toEqual([terminal(1, "a")]);
  state = reduceWatch(state, { type: "terminal_changed", summary: terminal(2, "b") });
  state = reduceWatch(state, { type: "terminal_changed", summary: terminal(3, "a") });
  expect(state.terminals).toEqual([terminal(1, "a"), terminal(2, "b"), terminal(3, "a")]);
  state = reduceWatch(state, {
    type: "terminal_changed",
    summary: terminal(2, "b", "npm run dev"),
  });
  expect(state.terminals).toEqual([
    terminal(1, "a"),
    terminal(2, "b", "npm run dev"),
    terminal(3, "a"),
  ]);
  state = reduceWatch(state, { type: "terminal_exited", terminal: 1 });
  expect(state.terminals).toEqual([terminal(2, "b", "npm run dev"), terminal(3, "a")]);
  state = reduceWatch(state, { type: "workspace_removed", name: "a" });
  expect(state.terminals).toEqual([terminal(2, "b", "npm run dev")]);
});

test("workspace_terminals_show_newest_first", () => {
  const state = reduceWatch(connected, {
    type: "watch_snapshot",
    terminals: [terminal(1, "a"), terminal(2, "b"), terminal(3, "a")],
    servers: [],
    config_error: null,
    workspaces: [
      { name: "a", path: "/a", color: "blue" },
      { name: "b", path: "/b", color: "blue" },
    ],
    sessions: [],
  });
  expect(workspaceTerminals(state, "a").map((summary) => summary.terminal)).toEqual([3, 1]);
});

test("has_snapshot_follows_the_connection", () => {
  expect(connected.hasSnapshot).toBe(false);
  let state = reduceWatch(connected, {
    type: "watch_snapshot",
    terminals: [],
    servers: [],
    config_error: null,
    workspaces: [],
    sessions: [],
  });
  expect(state.hasSnapshot).toBe(true);
  state = reduceWatch(state, { type: "connection", connected: false, socket: "/tmp/ur.sock", error: null });
  expect(state.hasSnapshot).toBe(false);
  // Reconnecting waits for the new connection's snapshot.
  state = reduceWatch(state, { type: "connection", connected: true, socket: "/tmp/ur.sock", error: null });
  expect(state.hasSnapshot).toBe(false);
});
