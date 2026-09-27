import { type ReactNode, useEffect, useRef } from "react";
import { createPortal } from "react-dom";

/**
 * A modal dialog in the middle of the window, open while mounted. Escape
 * calls `onClose`. It opens in the browser's top layer, above dockview's
 * dividers and drop targets, and is rendered into the document body so it
 * inherits no styles from where it is mounted.
 */
export function Dialog({ className, label, onClose, children }: {
  className: string;
  label: string;
  onClose: () => void;
  children: ReactNode;
}) {
  const dialog = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    dialog.current?.showModal();
  }, []);

  return createPortal(
    <dialog
      ref={dialog}
      className={"m-auto max-h-[calc(100%-3rem)] overflow-y-auto rounded-md border border-outline bg-raised p-5 text-sm text-fg shadow-lg backdrop:bg-black/50 " + className}
      aria-label={label}
      onCancel={(event) => {
        event.preventDefault();
        onClose();
      }}
    >
      {children}
    </dialog>,
    document.body,
  );
}
