import { type ReactNode, useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { addWorkspace, pickWorkspaceFolder } from "../actions";
import { workspaceColors } from "../colors";
import type { WorkspaceColor } from "../ipc/bindings/WorkspaceColor";

/**
 * A button that opens the folder picker, then the workspace color picker for
 * the chosen folder. Choosing a swatch adds the workspace.
 */
export function AddWorkspaceButton({
  className,
  title,
  children,
}: {
  className: string;
  title?: string;
  children: ReactNode;
}) {
  const [folder, setFolder] = useState<{ name: string; path: string } | null>(null);
  return (
    <>
      <button className={className} title={title} onClick={() => void pickWorkspaceFolder().then(setFolder)}>
        {children}
      </button>
      {folder !== null && (
        <WorkspaceColorPicker
          name={folder.name}
          onChoose={(color) => {
            setFolder(null);
            void addWorkspace(folder.name, folder.path, color);
          }}
          onClose={() => setFolder(null)}
        />
      )}
    </>
  );
}

/**
 * A dialog in the middle of the window with one swatch per workspace color.
 * Escape or a press on the backdrop closes it.
 */
export function WorkspaceColorPicker({
  name,
  onChoose,
  onClose,
}: {
  name: string;
  onChoose: (color: WorkspaceColor) => void;
  onClose: () => void;
}) {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        onClose();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  return createPortal(
    <div
      className="workspace-color-backdrop fixed inset-0 z-50 flex items-center justify-center bg-black/50"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) {
          onClose();
        }
      }}
    >
      <div className="workspace-color-picker w-72 rounded-md border border-outline bg-raised p-4 shadow-lg" role="dialog" aria-label="Workspace color">
        <div className="mb-3 truncate font-medium text-fg-strong" title={name}>{name}</div>
        <div className="grid grid-cols-7 gap-2">
          {(Object.entries(workspaceColors) as [WorkspaceColor, string][]).map(([color, label]) => (
            <button
              key={color}
              className="swatch size-7 rounded-full hover:ring-2 hover:ring-fg-strong focus-visible:ring-2 focus-visible:ring-fg-strong focus-visible:outline-none"
              style={{ backgroundColor: `var(--color-workspace-${color})` }}
              title={label}
              aria-label={label}
              onClick={() => onChoose(color)}
            />
          ))}
        </div>
      </div>
    </div>,
    document.body,
  );
}
