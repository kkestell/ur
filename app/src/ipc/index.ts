import { Channel, invoke } from "@tauri-apps/api/core";
import type { Request } from "./bindings/Request";
import type { Response } from "./bindings/Response";

export function request(request: Request): Promise<Response> {
  return invoke("request", { request });
}

/** Attaches the selected terminal, or a new one, and returns its ID. */
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
