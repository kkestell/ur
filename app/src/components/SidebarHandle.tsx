export const defaultSidebarWidth = 280;
export const minSidebarWidth = 160;
export const maxSidebarWidth = 600;

/**
 * The drag handle on the sidebar's right border. Dragging it calls `onResize`
 * with each new width, kept between `minSidebarWidth` and `maxSidebarWidth`,
 * and releasing it calls `onResizeEnd` with the final width.
 */
export function SidebarHandle({
  width,
  onResize,
  onResizeEnd,
}: {
  width: number;
  onResize: (width: number) => void;
  onResizeEnd: (width: number) => void;
}) {
  return (
    <div
      className="sidebar-handle absolute top-0 right-0 z-10 h-full w-1 cursor-ew-resize"
      onPointerDown={(event) => {
        // Stops the press from starting a text selection.
        event.preventDefault();
        const start = { x: event.clientX, width };
        const widthAt = (x: number) =>
          Math.min(maxSidebarWidth, Math.max(minSidebarWidth, start.width + x - start.x));
        const onMove = (event: PointerEvent) => onResize(widthAt(event.clientX));
        const onUp = (event: PointerEvent) => {
          document.removeEventListener("pointermove", onMove);
          document.removeEventListener("pointerup", onUp);
          onResizeEnd(widthAt(event.clientX));
        };
        document.addEventListener("pointermove", onMove);
        document.addEventListener("pointerup", onUp);
      }}
    />
  );
}
