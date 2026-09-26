import type { CSSProperties } from "react";
import type { WorkspaceColor } from "./ipc/bindings/WorkspaceColor";

/** Each workspace color's label, in swatch order. */
export const workspaceColors: Record<WorkspaceColor, string> = {
  rosewater: "Rosewater",
  flamingo: "Flamingo",
  pink: "Pink",
  mauve: "Mauve",
  red: "Red",
  maroon: "Maroon",
  peach: "Peach",
  yellow: "Yellow",
  green: "Green",
  teal: "Teal",
  sky: "Sky",
  sapphire: "Sapphire",
  blue: "Blue",
  lavender: "Lavender",
};

/**
 * The inline style that sets `--workspace-color`, which `.workspace-tab` and
 * `.workspace-content` read. Without a color, they use their default.
 */
export function workspaceColorStyle(color: WorkspaceColor | undefined): CSSProperties {
  return color === undefined
    ? {}
    : ({ "--workspace-color": `var(--color-workspace-${color})` } as CSSProperties);
}
