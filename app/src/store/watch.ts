import { useSyncExternalStore } from "react";
import { type Connection, type WatchEvent, onConnection, onWatch } from "../ipc";
import type { SessionSummary } from "../ipc/bindings/SessionSummary";
import type { Workspace } from "../ipc/bindings/Workspace";

export type WatchState = {
  connected: boolean;
  socket: string;
  workspaces: Workspace[];
  sessions: SessionSummary[];
};

export type ConnectionEvent = { type: "connection" } & Connection;

export const initialWatch: WatchState = {
  connected: false,
  socket: "",
  workspaces: [],
  sessions: [],
};

/**
 * Applies one watch or connection event. A disconnect clears the workspaces
 * and sessions; the next watch snapshot fills them again.
 */
export function reduceWatch(state: WatchState, event: WatchEvent | ConnectionEvent): WatchState {
  switch (event.type) {
    case "connection":
      return {
        ...state,
        connected: event.connected,
        socket: event.socket,
        workspaces: event.connected ? state.workspaces : [],
        sessions: event.connected ? state.sessions : [],
      };
    case "watch_snapshot":
      return { ...state, workspaces: event.workspaces, sessions: event.sessions };
    case "workspace_added":
      return { ...state, workspaces: [...state.workspaces, event.workspace] };
    case "workspace_removed":
      return {
        ...state,
        workspaces: state.workspaces.filter((workspace) => workspace.name !== event.name),
        sessions: state.sessions.filter((session) => session.workspace !== event.name),
      };
    case "session_changed": {
      const index = state.sessions.findIndex(
        (session) => session.session === event.summary.session,
      );
      const sessions = [...state.sessions];
      if (index === -1) {
        sessions.push(event.summary);
      } else {
        sessions[index] = event.summary;
      }
      return { ...state, sessions };
    }
  }
}

/** Whether the session is `needs_permission`, `failed`, or unread. */
export function needsAttention(summary: SessionSummary): boolean {
  return (
    summary.status.type === "needs_permission" ||
    summary.status.type === "failed" ||
    summary.unread
  );
}

/**
 * The workspace's sessions, those needing attention first, then by last
 * activity, newest first. A session without one was created during this ACP
 * connection and has had no `session_info_update`, so it counts as newest.
 * RFC 3339 timestamps from one server compare as strings.
 */
export function workspaceSessions(state: WatchState, workspace: string): SessionSummary[] {
  return state.sessions
    .filter((session) => session.workspace === workspace)
    .sort((a, b) => {
      const attention = Number(needsAttention(b)) - Number(needsAttention(a));
      if (attention !== 0) {
        return attention;
      }
      if (a.updated_at === null || b.updated_at === null) {
        return Number(a.updated_at !== null) - Number(b.updated_at !== null);
      }
      return b.updated_at.localeCompare(a.updated_at);
    });
}

/** The workspaces with a session needing attention first, each group in creation order. */
export function orderedWorkspaces(state: WatchState): Workspace[] {
  return [...state.workspaces].sort(
    (a, b) =>
      Number(attentionCount(state, b.name) > 0) - Number(attentionCount(state, a.name) > 0),
  );
}

/** How many of the workspace's sessions need attention. */
export function attentionCount(state: WatchState, workspace: string): number {
  return state.sessions.filter(
    (session) => session.workspace === workspace && needsAttention(session),
  ).length;
}

let state = initialWatch;
const listeners = new Set<() => void>();

export function apply(event: WatchEvent | ConnectionEvent): void {
  state = reduceWatch(state, event);
  for (const listener of listeners) {
    listener();
  }
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useWatch(): WatchState {
  return useSyncExternalStore(subscribe, () => state);
}

// Installed at module load, before any component requests the snapshot.
onWatch(apply).catch(console.error);
onConnection((connection) => apply({ type: "connection", ...connection })).catch(console.error);
