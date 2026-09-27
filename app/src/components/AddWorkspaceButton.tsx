import { open } from "@tauri-apps/plugin-dialog";
import { type ReactNode, useState } from "react";
import { workspaceColors } from "../colors";
import { request } from "../ipc";
import type { WorkspaceColor } from "../ipc/bindings/WorkspaceColor";
import { Dialog } from "./Dialog";

/** A button that opens the Add Workspace dialog. */
export function AddWorkspaceButton({
  className,
  title,
  children,
}: {
  className: string;
  title?: string;
  children: ReactNode;
}) {
  const [adding, setAdding] = useState(false);
  return (
    <>
      <button className={className} title={title} onClick={() => setAdding(true)}>
        {children}
      </button>
      {adding && <AddWorkspaceDialog onClose={() => setAdding(false)} />}
    </>
  );
}

const button = "rounded border border-control-edge bg-control px-3 py-1 hover:bg-control-hover disabled:opacity-50 disabled:hover:bg-control";

/**
 * A modal dialog with the workspace's path, a Choose… button that opens the
 * folder picker, the workspace color swatches, and Cancel and Add. Escape
 * cancels. The workspace is named after the path's last component.
 */
function AddWorkspaceDialog({ onClose }: { onClose: () => void }) {
  const [path, setPath] = useState("");
  const [color, setColor] = useState<WorkspaceColor | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);

  async function choose() {
    try {
      const chosen = await open({ directory: true, defaultPath: path || undefined });
      if (chosen !== null) setPath(chosen);
    } catch (cause) { setError(String(cause)); }
  }

  async function add() {
    if (color === null) return;
    const name = path.split("/").filter((part) => part !== "").pop() ?? path;
    setAdding(true);
    setError(null);
    try {
      const response = await request({ type: "add_workspace", name, path, color });
      if (response.type === "error") setError(response.message);
      else onClose();
    } catch (cause) { setError(String(cause)); }
    finally { setAdding(false); }
  }

  return (
    <Dialog className="add-workspace w-96" label="Add Workspace" onClose={onClose}>
      <form
        className="flex flex-col gap-4"
        onSubmit={(event) => {
          event.preventDefault();
          void add();
        }}
      >
        <h2 className="font-semibold text-fg-strong">Add Workspace</h2>
        <label className="flex flex-col gap-1">Path
          <span className="flex gap-2">
            <input
              className="workspace-path min-w-0 flex-1 rounded bg-control px-2 py-1"
              value={path}
              autoFocus
              spellCheck={false}
              onChange={(event) => setPath(event.target.value)}
            />
            <button type="button" className={"workspace-choose " + button} onClick={() => void choose()}>Choose…</button>
          </span>
        </label>
        <div className="flex flex-col gap-2">
          <span id="workspace-color-label">Color</span>
          <div className="grid grid-cols-7 gap-2 justify-self-start" role="radiogroup" aria-labelledby="workspace-color-label">
            {(Object.entries(workspaceColors) as [WorkspaceColor, string][]).map(([value, label]) => (
              <button
                key={value}
                type="button"
                role="radio"
                aria-checked={color === value}
                className={
                  "swatch size-7 rounded-full ring-offset-2 ring-offset-raised hover:ring-2 hover:ring-fg-muted focus-visible:ring-2 focus-visible:ring-fg-strong focus-visible:outline-none" +
                  (color === value ? " selected ring-2 ring-fg-strong hover:ring-fg-strong" : "")
                }
                style={{ backgroundColor: `var(--color-workspace-${value})` }}
                title={label}
                aria-label={label}
                onClick={() => setColor(value)}
              />
            ))}
          </div>
        </div>
        {error !== null && <p className="workspace-error text-danger">{error}</p>}
        <div className="flex justify-end gap-2">
          <button type="button" className={"workspace-cancel " + button} onClick={onClose}>Cancel</button>
          <button type="submit" className={"workspace-add " + button} disabled={adding || path.trim() === "" || color === null}>
            {adding ? "Adding…" : "Add"}
          </button>
        </div>
      </form>
    </Dialog>
  );
}
