import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Connection } from "./bindings/Connection";
import type { Event } from "./bindings/Event";
import type { Request } from "./bindings/Request";
import type { Response } from "./bindings/Response";
import type { Selection } from "./bindings/Selection";

export type { Connection, Selection };

/** The events the core forwards under the `watch` event name. */
export type WatchEvent = Extract<
  Event,
  {
    type:
      | "watch_snapshot"
      | "capabilities_changed"
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

export function selection(): Promise<Selection | null> {
  return invoke("selection");
}

export function select(selection: Selection): Promise<void> {
  return invoke("select", { selection });
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
