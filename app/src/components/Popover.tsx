import { type ReactNode, type RefObject, useEffect, useLayoutEffect, useRef } from "react";
import { createPortal } from "react-dom";

/** The space between a popover and its anchor. */
const GAP = 4;
/** The closest a popover comes to the window's edges. */
const MARGIN = 8;

/**
 * A popup above `anchor`, lined up with its start, center, or end and kept
 * inside the window. It renders into the document body, because each pane
 * clips its content.
 */
export function Popover({
  anchor,
  align,
  className,
  children,
}: {
  anchor: RefObject<HTMLElement | null>;
  align: "start" | "center" | "end";
  className: string;
  children: ReactNode;
}) {
  const popover = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    const element = popover.current!;
    const place = () => {
      const target = anchor.current!.getBoundingClientRect();
      const { width, height } = element.getBoundingClientRect();
      const left =
        align === "start"
          ? target.left
          : align === "end"
            ? target.right - width
            : target.left + (target.width - width) / 2;
      element.style.left = `${Math.max(MARGIN, Math.min(left, window.innerWidth - width - MARGIN))}px`;
      element.style.top = `${Math.max(MARGIN, target.top - height - GAP)}px`;
    };
    place();
    // A filtered list changes height, and stays above its anchor.
    const observer = new ResizeObserver(place);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  return createPortal(
    <div ref={popover} className={"popover fixed z-50 rounded-md border border-outline bg-raised shadow-lg " + className}>
      {children}
    </div>,
    document.body,
  );
}

/**
 * While `open`, calls `close` on a mouse press outside the element given the
 * returned props. A press in a popover inside it counts as inside: React
 * passes the press up through the portal to the element before the window
 * sees it.
 */
export function useClickOutside(open: boolean, close: () => void): { onMouseDown: () => void } {
  const inside = useRef(false);
  useEffect(() => {
    if (!open) {
      return;
    }
    const onMouseDown = () => {
      if (!inside.current) {
        close();
      }
      inside.current = false;
    };
    window.addEventListener("mousedown", onMouseDown);
    return () => window.removeEventListener("mousedown", onMouseDown);
  }, [open]);
  return {
    onMouseDown: () => {
      // Only a press while open is one the window listener must ignore; the
      // press that opened the popover happens before the listener is installed.
      if (open) {
        inside.current = true;
      }
    },
  };
}
