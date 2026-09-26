import type { AgentCapabilities } from "@agentclientprotocol/sdk";
import { useSyncExternalStore } from "react";
import { type Connection, type WatchEvent, onConnection, onWatch } from "../ipc";
import type { SessionSummary } from "../ipc/bindings/SessionSummary";
import type { TerminalSummary } from "../ipc/bindings/TerminalSummary";
import type { Workspace } from "../ipc/bindings/Workspace";

export type WatchState = {
  connected: boolean;
  /** Whether the watch snapshot of the current connection has arrived. */
  hasSnapshot: boolean;
  socket: string;
  workspaces: Workspace[];
  sessions: SessionSummary[];
  /** In the order they were opened. */
  terminals: TerminalSummary[];
  /** The current ACP connection's capabilities, or `null` without one. */
  capabilities: AgentCapabilities | null;
};

export type ConnectionEvent = { type: "connection" } & Connection;

export const initialWatch: WatchState = {
  connected: false,
  hasSnapshot: false,
  socket: "",
  workspaces: [],
  sessions: [],
  terminals: [],
  capabilities: null,
};

/**
 * Applies one watch or connection event. A disconnect clears the workspaces,
 * sessions, terminals, capabilities, and `hasSnapshot`; the next watch
 * snapshot fills them again.
 */
export function reduceWatch(state: WatchState, event: WatchEvent | ConnectionEvent): WatchState {
  switch (event.type) {
    case "connection":
      return {
        ...state,
        connected: event.connected,
        hasSnapshot: event.connected && state.hasSnapshot,
        socket: event.socket,
        workspaces: event.connected ? state.workspaces : [],
        sessions: event.connected ? state.sessions : [],
        terminals: event.connected ? state.terminals : [],
        capabilities: event.connected ? state.capabilities : null,
      };
    case "watch_snapshot":
      return {
        ...state,
        hasSnapshot: true,
        workspaces: event.workspaces,
        sessions: event.sessions,
        terminals: event.terminals,
        capabilities: event.capabilities,
      };
    case "capabilities_changed":
      return { ...state, capabilities: event.capabilities };
    case "workspace_added":
      return { ...state, workspaces: [...state.workspaces, event.workspace] };
    case "workspace_removed":
      return {
        ...state,
        workspaces: state.workspaces.filter((workspace) => workspace.name !== event.name),
        sessions: state.sessions.filter((session) => session.workspace !== event.name),
        terminals: state.terminals.filter((terminal) => terminal.workspace !== event.name),
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
    case "session_deleted":
      return {
        ...state,
        sessions: state.sessions.filter((session) => session.session !== event.session),
      };
    case "terminal_changed": {
      const index = state.terminals.findIndex(
        (terminal) => terminal.terminal === event.summary.terminal,
      );
      const terminals = [...state.terminals];
      if (index === -1) {
        terminals.push(event.summary);
      } else {
        terminals[index] = event.summary;
      }
      return { ...state, terminals };
    }
    case "terminal_exited":
      return {
        ...state,
        terminals: state.terminals.filter((terminal) => terminal.terminal !== event.terminal),
      };
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

/** The workspace's terminals in the order they were opened. */
export function workspaceTerminals(state: WatchState, workspace: string): TerminalSummary[] {
  return state.terminals.filter((terminal) => terminal.workspace === workspace);
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
