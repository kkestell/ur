import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { SerializedDockview } from "dockview-react";
import type { Connection } from "./bindings/Connection";
import type { Event } from "./bindings/Event";
import type { Request } from "./bindings/Request";
import type { Response } from "./bindings/Response";

export type { Connection };

/** The events the core forwards under the `watch` event name. */
export type WatchEvent = Extract<
  Event,
  {
    type:
      | "watch_snapshot"
      | "capabilities_changed"
      | "server_state_changed"
      | "workspace_added"
      | "workspace_removed"
      | "session_changed"
      | "session_deleted"
      | "terminal_changed"
      | "terminal_exited";
  }
>;

/** The events the core forwards under the `session` event name. */
export type SessionEvent = Extract<
  Event,
  { type: "session_snapshot" | "entry" | "config_options_changed" | "session_removed" }
>;

export function request(request: Request): Promise<Response> {
  return invoke("request", { request });
}

/** Attaches the terminal: its screen snapshot, then its live output. */
export function attachTerminal(
  terminal: number,
  rows: number,
  cols: number,
  onOutput: (bytes: Uint8Array) => void,
): Promise<void> {
  const output = new Channel<ArrayBuffer>();
  output.onmessage = (bytes) => onOutput(new Uint8Array(bytes));
  return invoke("attach_terminal", { terminal, rows, cols, output });
}

/** Ends the terminal attachment. The terminal keeps running. */
export function detachTerminal(terminal: number): Promise<void> {
  return invoke("detach_terminal", { terminal });
}

export function terminalInput(terminal: number, data: string): Promise<void> {
  return invoke("terminal_input", { terminal, data });
}

/** The current connection, since a `connection` event before the listener is lost. */
export function connection(): Promise<Connection> {
  return invoke("connection");
}

/** The saved layout, or `null` when none is saved. */
export function layout(): Promise<SerializedDockview | null> {
  return invoke("layout");
}

export function saveLayout(layout: SerializedDockview): Promise<void> {
  return invoke("save_layout", { layout });
}

/** The saved sidebar width in pixels, or `null` when none is saved. */
export function sidebarWidth(): Promise<number | null> {
  return invoke("sidebar_width");
}

export function saveSidebarWidth(width: number): Promise<void> {
  return invoke("save_sidebar_width", { width });
}

/** Reports the sessions shown in the window; the core sends `focus` from them. */
export function setVisible(sessions: string[]): Promise<void> {
  return invoke("set_visible", { sessions });
}

export function onWatch(listener: (event: WatchEvent) => void): Promise<() => void> {
  return listen<WatchEvent>("watch", (event) => listener(event.payload));
}

export function onSession(listener: (event: SessionEvent) => void): Promise<() => void> {
  return listen<SessionEvent>("session", (event) => listener(event.payload));
}

export function onConnection(listener: (connection: Connection) => void): Promise<() => void> {
  return listen<Connection>("connection", (event) => listener(event.payload));
}
