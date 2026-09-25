import { expect, test } from "vitest";
import { type Keys, shortcutKind, shortcutLabel } from "./keys";

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
