import { Menu } from "@tauri-apps/api/menu";
import { ask, message, open } from "@tauri-apps/plugin-dialog";
import { type Selection, request } from "./ipc";
import type { Request } from "./ipc/bindings/Request";
import type { Response } from "./ipc/bindings/Response";
import type { SessionSummary } from "./ipc/bindings/SessionSummary";
import type { Status } from "./ipc/bindings/Status";
import type { TerminalSummary } from "./ipc/bindings/TerminalSummary";
import type { WatchState } from "./store/watch";

/**
 * Picks a folder and adds it as a workspace named after its last path
 * component. Cancelling the picker does nothing.
 */
export async function addWorkspace(): Promise<void> {
  const path = await open({ directory: true });
  if (path === null) {
    return;
  }
  const name = path.split("/").filter((part) => part !== "").pop() ?? path;
  await send({ type: "add_workspace", name, path });
}

/** Creates a session in the workspace, then selects it. */
export async function newSession(
  workspace: string,
  onSelect: (selection: Selection) => void,
): Promise<void> {
  const response = await send({ type: "new_session", workspace });
  if (response?.type === "session_created") {
    onSelect({ type: "session", session: response.session });
  }
}

/** Opens a terminal in the workspace, then selects it. */
export async function newTerminal(
  workspace: string,
  onSelect: (selection: Selection) => void,
): Promise<void> {
  const response = await send({ type: "open_terminal", workspace });
  if (response?.type === "opened") {
    onSelect({ type: "terminal", terminal: response.terminal });
  }
}

/** Stops the terminal's shell. The terminal leaves the sidebar when it exits. */
export async function closeTerminal(terminal: number): Promise<void> {
  await send({ type: "close_terminal", terminal });
}

/** Asks for confirmation, then removes the workspace. */
export async function removeWorkspace(watch: WatchState, name: string): Promise<void> {
  const running = watch.sessions.some(
    (session) => session.workspace === name && isRunning(session.status),
  );
  const terminals = watch.terminals.some((terminal) => terminal.workspace === name);
  const confirmed = await ask(
    lines(
      `Remove "${name}"?`,
      running && "Its running turns will be cancelled.",
      terminals && "Its terminals will be closed.",
    ),
    { title: "Remove workspace?", kind: "warning", okLabel: "Remove" },
  );
  if (confirmed) {
    await send({ type: "remove_workspace", name });
  }
}

/** Asks for confirmation, then deletes the session. */
export async function deleteSession(summary: SessionSummary): Promise<void> {
  const title = summary.title ?? "New session";
  const confirmed = await ask(
    lines(
      `Delete "${title}" and its saved transcript?`,
      isRunning(summary.status) && "Its running turn will be cancelled.",
    ),
    { title: "Delete session?", kind: "warning", okLabel: "Delete" },
  );
  if (confirmed) {
    await send({ type: "delete_session", session: summary.session });
  }
}

/** The workspace menu: New Session, New Terminal, and Remove Workspace…. */
export async function showWorkspaceMenu(
  watch: WatchState,
  workspace: string,
  onSelect: (selection: Selection) => void,
): Promise<void> {
  const menu = await Menu.new({
    items: [
      { text: "New Session", action: () => void newSession(workspace, onSelect) },
      { text: "New Terminal", action: () => void newTerminal(workspace, onSelect) },
      { item: "Separator" },
      { text: "Remove Workspace…", action: () => void removeWorkspace(watch, workspace) },
    ],
  });
  await menu.popup();
}

/** The session menu: Delete…. */
export async function showSessionMenu(summary: SessionSummary): Promise<void> {
  const menu = await Menu.new({
    items: [{ text: "Delete…", action: () => void deleteSession(summary) }],
  });
  await menu.popup();
}

/** The terminal menu: Close Terminal. */
export async function showTerminalMenu(summary: TerminalSummary): Promise<void> {
  const menu = await Menu.new({
    items: [{ text: "Close Terminal", action: () => void closeTerminal(summary.terminal) }],
  });
  await menu.popup();
}

function isRunning(status: Status): boolean {
  return status.type === "working" || status.type === "needs_permission";
}

function lines(...parts: (string | false)[]): string {
  return parts.filter((part) => part !== false).join("\n\n");
}

/**
 * Sends the request and returns its response. A busy or error response, or a
 * failed request, shows in an error dialog and returns `undefined`.
 */
async function send(req: Request): Promise<Response | undefined> {
  let failure: string;
  try {
    const response = await request(req);
    if (response.type === "busy") {
      failure = "The session is busy.";
    } else if (response.type === "error") {
      failure = response.message;
    } else {
      return response;
    }
  } catch (error) {
    failure = String(error);
  }
  await message(failure, { kind: "error" });
  return undefined;
}
