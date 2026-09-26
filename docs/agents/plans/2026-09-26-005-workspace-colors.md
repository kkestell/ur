# Workspace colors

## Goal

Every workspace has a workspace color, one of the fourteen Catppuccin Mocha accent colors. Adding a
workspace requires choosing its color from a list of swatches after the folder picker. Each tab's
background is a darker version of its workspace's color, and the tab and its content have a 1px
border in the workspace color.

## Related code

- `crates/ur-client/src/protocol.rs` — `Workspace` and `Request::AddWorkspace`, which gain the
  color.
- `crates/ur/src/daemon/server.rs` — builds the `Workspace` from `AddWorkspace`.
- `crates/ur/src/daemon/state_file.rs` — serializes `Workspace` unchanged, so the color reaches the
  state file with no change here.
- `crates/ur/src/daemon/tests.rs` — `workspace()`, `add_workspace()`,
  `workspaces_survive_a_daemon_restart`, and `workspace_requests_reject_bad_input`.
- `app/src/actions.ts` — `addWorkspace()`, which opens the folder picker and adds the workspace.
- `app/src/components/Sidebar.tsx` — the header's `+`, which calls `addWorkspace()`.
- `app/src/components/Layout.tsx` — the watermark's Add Workspace button, `SessionPanel`, and
  `TerminalPanel`.
- `app/src/components/Tab.tsx` — the tab component, rendered inside dockview's `.dv-tab`.
- `app/src/layout.ts` — `tabWorkspace()`, which finds a tab's workspace.
- `app/src/styles.css` — the `@theme` block and the dockview tab rules: `.dv-tab` padding comes from
  dockview (`0.25rem 0.5rem`), and the tab bar's bottom line comes from the `.dv-tab`,
  `.dv-void-container`, and `.dv-right-actions-container` borders.
- `app/e2e/dialog-shim.ts` — re-exports the real `open`, which WebDriver cannot answer.
- `app/e2e/harness.ts` — `TestEnvironment.addWorkspace()` and the dialog state type.

## Decisions

- The daemon owns the workspace color, because it owns workspaces. `Workspace` gains
  `color: WorkspaceColor`, so the state file, `workspace_added`, and the watch snapshot carry it
  with no other daemon change. An existing `state.json` without colors fails to parse; delete it, as
  `AGENTS.md` directs.
- `WorkspaceColor` is a Rust enum of the fourteen accent names, serialized in snake case, so the
  wire protocol rejects anything else and ts-rs exports it as a string union. The daemon never needs
  the color values. The swatch order is Catppuccin's: rosewater, flamingo, pink, mauve, red, maroon,
  peach, yellow, green, teal, sky, sapphire, blue, lavender.
- The color values live in the `@theme` block as `--color-workspace-<name>`, with Catppuccin Mocha's
  values: `#f5e0dc`, `#f2cdcd`, `#f5c2e7`, `#cba6f7`, `#f38ba8`, `#eba0ac`, `#fab387`, `#f9e2af`,
  `#a6e3a1`, `#94e2d5`, `#89dceb`, `#74c7ec`, `#89b4fa`, `#b4befe`. The rest of the app's theme
  stays as it is.
- The tab and its content read the color from a `--workspace-color` custom property set inline on
  their element. Two rules in `styles.css` use it: `.workspace-tab` has the background
  `color-mix(in srgb, var(--workspace-color) 25%, black)` and a 1px border on its top, left, and
  right; `.workspace-content` has a 1px border on all sides. The content's top border is the line
  under the tabs, so the tab bar's own bottom line and the tab dividers go away. Both rules default
  `--workspace-color` to `var(--color-divider)` for a terminal tab opened before its
  `terminal_changed` arrives, when the tab has no workspace yet.
- `.dv-tab` loses its padding so the `Tab` component's root fills it and paints the whole tab. The
  root takes over the padding with `px-2`. Hidden tabs keep dockview's dimmer text color, which
  distinguishes them from the active tab now that every tab has a workspace background.
- The workspace color picker is a dialog in the middle of the window, rendered into the document
  body over a dimmed backdrop, since it opens from both the sidebar header and the watermark. It
  shows the workspace name and one swatch button per color, each with the color's name as its
  `title` and accessible name. Choosing a swatch adds the workspace and closes the picker. Escape or
  a press on the backdrop closes it without adding anything. There is no default color.
- `AddWorkspaceButton` owns the whole flow: its click opens the folder picker, then the workspace
  color picker for the chosen folder. The sidebar and the watermark each render one, so no state
  moves into `App`.

## Naming

- **Workspace color** — The Catppuccin Mocha accent color chosen for a workspace when it is added.
  `WorkspaceColor` in Rust and TypeScript, `Workspace.color` on the wire and in the state file, and
  `--color-workspace-<name>` in `styles.css`.
- **Swatch** — One button in the workspace color picker showing one workspace color.
- **Workspace color picker** — The dialog of swatches shown after the folder picker when adding a
  workspace. `WorkspaceColorPicker` in code.
- **Workspace** — Gains its workspace color: a name, a workspace path, and a workspace color.

## Test plan

- Daemon: `workspace()` and `add_workspace()` in `tests.rs` include a workspace color, so
  `workspaces_survive_a_daemon_restart` checks that the color survives the state file.
- End-to-end, in `app/e2e/workspace.test.ts`: with the folder picker answered with a temporary
  directory, clicking the sidebar's Add Workspace shows the workspace color picker with fourteen
  swatches. Escape closes it, and no workspace row for the directory appears. Opening it again and
  choosing Mauve adds the workspace's row. Guarantee: "Adding a workspace asks for its workspace
  color; choosing a swatch adds the workspace, and dismissing the workspace color picker adds
  nothing."
- The tab and content colors are styling and get no end-to-end test. Check them with an ad-hoc check
  or `make run`.

## Implementation plan

1. `crates/ur-client/src/protocol.rs`: add `WorkspaceColor`
   (`Clone, Copy, Debug, PartialEq,
   Serialize, Deserialize, TS`,
   `#[serde(rename_all = "snake_case")]`, `#[ts(export)]`) with the fourteen variants in swatch
   order. Add `color: WorkspaceColor` to `Workspace` and to `Request::AddWorkspace`, and document it
   on the request.
2. `crates/ur/src/daemon/server.rs`: pass `color` into the `Workspace`.
3. `crates/ur/src/daemon/tests.rs`: add `color: WorkspaceColor::Blue` to `workspace()`,
   `add_workspace()`, and the `add` closure in `workspace_requests_reject_bad_input`. Run
   `cargo test` to regenerate `app/src/ipc/bindings/`, including `WorkspaceColor.ts`.
4. `app/src/styles.css`: add the fourteen `--color-workspace-<name>` values to `@theme` under a
   "Workspace colors" comment. Add `.workspace-tab` and `.workspace-content`. Set `.panes .dv-tab`
   padding to 0, and remove the `.dv-tab` right and bottom borders, the `.dv-active-tab` rule, and
   the `.dv-void-container, .dv-right-actions-container` bottom border. Update the comment above
   `.panes` to describe the new tab bar.
5. `app/src/colors.ts` (new): `workspaceColors`, a `Record<WorkspaceColor, string>` from each color
   to its label ("Rosewater", …), in swatch order, and `workspaceColorStyle(color)`, which returns
   the inline style setting `--workspace-color` to `var(--color-workspace-<color>)`, or no property
   for `undefined`.
6. `app/src/layout.ts`: add `tabColor(watch, item)`, the workspace color of `tabWorkspace()`'s
   workspace.
7. `app/src/components/Tab.tsx`: add `workspace-tab` and `px-2` to the root, replacing `px-1`, and
   set its style from `workspaceColorStyle(tabColor(watch, params))`.
8. `app/src/components/Layout.tsx`: give `SessionPanel`'s root and a new wrapper around
   `TerminalPane` in `TerminalPanel` the `workspace-content` class and the same style.
9. `app/src/actions.ts`: replace `addWorkspace()` with `pickWorkspaceFolder()`, which opens the
   folder picker and returns `{ name, path }` or `null`, and `addWorkspace(name, path, color)`,
   which sends `add_workspace`.
10. `app/src/components/AddWorkspaceButton.tsx` (new):
    `AddWorkspaceButton({ className, title,
    children })` and
    `WorkspaceColorPicker({ name, onChoose, onClose })`, as described under Decisions. The picker
    listens for Escape on `window` while open.
11. `app/src/components/Sidebar.tsx` and the watermark in `Layout.tsx`: use `AddWorkspaceButton`
    with their current classes and contents.
12. `app/e2e/dialog-shim.ts`: export an `open` that returns and clears `folder` from
    `window.__UR_E2E_DIALOG__`, and add `folder` to `DialogState`. `app/e2e/harness.ts`: add
    `folder` to the dialog state type, add `Gui.chooseFolder(path)`, which sets it, and give
    `TestEnvironment.addWorkspace()` a `color` parameter that defaults to `"blue"`.
13. `app/e2e/workspace.test.ts` (new): the end-to-end test above.

## Documentation updates

- `AGENTS.md`: add `WorkspaceColor` to `protocol.rs`; add `app/src/colors.ts`,
  `app/src/components/AddWorkspaceButton.tsx`, and `app/e2e/workspace.test.ts`; update the entries
  for `actions.ts`, `layout.ts`, `Sidebar.tsx`, `Layout.tsx`, `Tab.tsx`, `styles.css`,
  `dialog-shim.ts` (it also answers the folder picker), and `harness.ts`.
- `docs/agents/architecture.md`: the state file holds workspace names, absolute paths, and workspace
  colors. Under Sessions, tabs, and workspaces, describe choosing the workspace color after the
  folder picker, the tab background, and the borders on the tab and its content.
- `docs/agents/glossary.md`: add workspace color, swatch, and workspace color picker; update
  Workspace and State file.
- `docs/agents/testing.md`: add the guarantee to the end-to-end guarantee list.
- `README.md`: in Get started, adding a workspace with the **+** asks for its color.
