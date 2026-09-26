import type { PermissionOptionKind } from "@agentclientprotocol/sdk";

/**
 * The shortcut for each permission option kind. A shortcut answers the
 * selected session's oldest pending request with its first option of that
 * kind, like `ur approve` and `ur deny`.
 */
const shortcuts: { kind: PermissionOptionKind; key: string; shift: boolean; alt: boolean }[] = [
  { kind: "allow_once", key: "y", shift: false, alt: false },
  { kind: "allow_always", key: "y", shift: true, alt: false },
  { kind: "reject_once", key: "z", shift: false, alt: true },
  { kind: "reject_always", key: "z", shift: true, alt: true },
];

/** The parts of a keyboard event a shortcut reads. */
export type Keys = Pick<KeyboardEvent, "code" | "metaKey" | "ctrlKey" | "shiftKey" | "altKey">;

/** The option kind the keyboard event's shortcut answers with, if any. */
export function shortcutKind(event: Keys): PermissionOptionKind | undefined {
  if (!event.metaKey || event.ctrlKey) {
    return undefined;
  }
  // `code` names the physical key, since ⌥ changes `key` on macOS.
  const key = event.code === "KeyY" ? "y" : event.code === "KeyZ" ? "z" : undefined;
  return shortcuts.find(
    (shortcut) =>
      shortcut.key === key && shortcut.shift === event.shiftKey && shortcut.alt === event.altKey,
  )?.kind;
}

/** The shortcut a permission option row shows for its kind. */
export function shortcutLabel(kind: PermissionOptionKind): string {
  const shortcut = shortcuts.find((shortcut) => shortcut.kind === kind)!;
  return (shortcut.shift ? "⇧" : "") + (shortcut.alt ? "⌥" : "") + "⌘" + shortcut.key.toUpperCase();
}

/** What ⌘N or ⇧⌘N opens: a new session or a new terminal. */
export function newShortcut(event: Keys): "session" | "terminal" | undefined {
  if (!event.metaKey || event.ctrlKey || event.altKey || event.code !== "KeyN") {
    return undefined;
  }
  return event.shiftKey ? "terminal" : "session";
}

/** The tab action for the platform's Command or Ctrl shortcut. */
export function tabShortcut(event: Keys, mac: boolean): "close" | "reopen" | undefined {
  if (event.metaKey !== mac || event.ctrlKey === mac || event.altKey) {
    return undefined;
  }
  if (event.code === "KeyW" && !event.shiftKey) {
    return "close";
  }
  return event.code === "KeyT" && event.shiftKey ? "reopen" : undefined;
}
