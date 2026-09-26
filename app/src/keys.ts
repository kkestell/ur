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

/** macOS uses Command; other platforms use Ctrl for app shortcuts. */
export function isMacPlatform(): boolean {
  return navigator.platform.startsWith("Mac");
}

function primaryModifier(event: Keys, mac: boolean): boolean {
  return event.metaKey === mac && event.ctrlKey !== mac;
}

/** The option kind the keyboard event's shortcut answers with, if any. */
export function shortcutKind(event: Keys, mac: boolean): PermissionOptionKind | undefined {
  if (!primaryModifier(event, mac)) {
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
export function shortcutLabel(kind: PermissionOptionKind, mac: boolean): string {
  const shortcut = shortcuts.find((shortcut) => shortcut.kind === kind)!;
  return mac
    ? (shortcut.shift ? "⇧" : "") + (shortcut.alt ? "⌥" : "") + "⌘" + shortcut.key.toUpperCase()
    : "Ctrl+" + (shortcut.shift ? "Shift+" : "") + (shortcut.alt ? "Alt+" : "") + shortcut.key.toUpperCase();
}

/** What the platform's New Session or New Terminal shortcut opens. */
export function newShortcut(event: Keys, mac: boolean): "session" | "terminal" | undefined {
  if (!primaryModifier(event, mac) || event.altKey || event.code !== "KeyN") {
    return undefined;
  }
  return event.shiftKey ? "terminal" : "session";
}

/** The tab action for the platform's Command or Ctrl shortcut. */
export function tabShortcut(event: Keys, mac: boolean): "close" | "reopen" | undefined {
  if (!primaryModifier(event, mac) || event.altKey) {
    return undefined;
  }
  if (event.code === "KeyW" && !event.shiftKey) {
    return "close";
  }
  return event.code === "KeyT" && event.shiftKey ? "reopen" : undefined;
}
