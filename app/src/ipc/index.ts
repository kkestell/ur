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
  { type: "watch_snapshot" | "workspace_added" | "workspace_removed" | "session_changed" }
>;

/** The events the core forwards under the `session` event name. */
export type SessionEvent = Extract<
  Event,
  { type: "session_snapshot" | "entry" | "session_removed" }
>;

export function request(request: Request): Promise<Response> {
  return invoke("request", { request });
}

/** Attaches the GUI's terminal, or a new one, and returns its ID. */
export function attachTerminal(
  rows: number,
  cols: number,
  onOutput: (bytes: Uint8Array) => void,
): Promise<number> {
  const output = new Channel<ArrayBuffer>();
  output.onmessage = (bytes) => onOutput(new Uint8Array(bytes));
  return invoke("attach_terminal", { rows, cols, output });
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

export function onWatch(listener: (event: WatchEvent) => void): Promise<() => void> {
  return listen<WatchEvent>("watch", (event) => listener(event.payload));
}

export function onSession(listener: (event: SessionEvent) => void): Promise<() => void> {
  return listen<SessionEvent>("session", (event) => listener(event.payload));
}

export function onConnection(listener: (connection: Connection) => void): Promise<() => void> {
  return listen<Connection>("connection", (event) => listener(event.payload));
}
