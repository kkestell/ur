import { expect, test } from "vitest";
import { type Keys, newShortcut, shortcutKind, shortcutLabel, tabShortcut } from "./keys";

function keys(init: Partial<Keys>): Keys {
  return { code: "", metaKey: false, ctrlKey: false, shiftKey: false, altKey: false, ...init };
}

test("shortcuts_map_to_option_kinds", () => {
  const cases = [
    { init: { code: "KeyY", metaKey: true }, kind: "allow_once", label: "⌘Y" },
    { init: { code: "KeyY", metaKey: true, shiftKey: true }, kind: "allow_always", label: "⇧⌘Y" },
    { init: { code: "KeyZ", metaKey: true, altKey: true }, kind: "reject_once", label: "⌥⌘Z" },
    {
      init: { code: "KeyZ", metaKey: true, altKey: true, shiftKey: true },
      kind: "reject_always",
      label: "⇧⌥⌘Z",
    },
    { init: { code: "KeyY" }, kind: undefined, label: undefined },
  ] as const;
  for (const { init, kind, label } of cases) {
    expect(shortcutKind(keys(init)), JSON.stringify(init)).toBe(kind);
    if (kind !== undefined) {
      expect(shortcutLabel(kind), kind).toBe(label);
    }
  }
});

test("new_shortcuts_map_to_sessions_and_terminals", () => {
  const cases = [
    { init: { code: "KeyN", metaKey: true }, opens: "session" },
    { init: { code: "KeyN", metaKey: true, shiftKey: true }, opens: "terminal" },
    { init: { code: "KeyT", metaKey: true }, opens: undefined },
    { init: { code: "KeyT", metaKey: true, shiftKey: true }, opens: undefined },
    { init: { code: "KeyN", metaKey: true, altKey: true }, opens: undefined },
    { init: { code: "KeyN" }, opens: undefined },
  ] as const;
  for (const { init, opens } of cases) {
    expect(newShortcut(keys(init)), JSON.stringify(init)).toBe(opens);
  }
});

test("tab_shortcuts_match_platform_and_exact_modifiers", () => {
  const cases = [
    { init: { code: "KeyW", metaKey: true }, mac: true, action: "close" },
    { init: { code: "KeyT", metaKey: true, shiftKey: true }, mac: true, action: "reopen" },
    { init: { code: "KeyW", ctrlKey: true }, mac: false, action: "close" },
    { init: { code: "KeyT", ctrlKey: true, shiftKey: true }, mac: false, action: "reopen" },
    { init: { code: "KeyW", ctrlKey: true }, mac: true, action: undefined },
    { init: { code: "KeyW", metaKey: true }, mac: false, action: undefined },
    { init: { code: "KeyW", metaKey: true, shiftKey: true }, mac: true, action: undefined },
    { init: { code: "KeyT", metaKey: true, shiftKey: true, altKey: true }, mac: true, action: undefined },
    { init: { code: "KeyW", metaKey: true, ctrlKey: true }, mac: true, action: undefined },
  ] as const;
  for (const { init, mac, action } of cases) {
    expect(tabShortcut(keys(init), mac), JSON.stringify({ init, mac })).toBe(action);
  }
});
