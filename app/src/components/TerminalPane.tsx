import { useEffect, useRef, useState } from "react";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { attachTerminal, detachTerminal, request, terminalInput } from "../ipc";

/**
 * The terminal's xterm.js view. Attaches on mount and detaches on unmount, so
 * render it keyed by terminal ID.
 */
export function TerminalPane({ terminal }: { terminal: number }) {
  const container = useRef<HTMLDivElement>(null);
  const [error, setError] = useState<string>();

  useEffect(() => {
    // xterm.js draws on a canvas, so it takes the theme's colors as values.
    const theme = getComputedStyle(document.documentElement);
    const xterm = new Terminal({
      fontFamily: "Menlo, monospace",
      fontSize: 13,
      theme: {
        background: theme.getPropertyValue("--color-surface"),
        foreground: theme.getPropertyValue("--color-fg"),
      },
    });
    const fit = new FitAddon();
    xterm.loadAddon(fit);
    const element = container.current!;
    xterm.open(element);
    // A tab that is not active has no size, and fitting it would shrink the
    // terminal to one row and resize the PTY.
    const fitIfShown = () => {
      if (element.clientWidth > 0 && element.clientHeight > 0) {
        fit.fit();
      }
    };
    fitIfShown();

    let attached = false;
    const resize = () => {
      if (attached) {
        const { rows, cols } = xterm;
        request({ type: "terminal_resize", terminal, rows, cols }).catch(console.error);
      }
    };
    const { rows, cols } = xterm;
    const attaching = attachTerminal(terminal, rows, cols, (bytes) => xterm.write(bytes)).then(
      () => {
        attached = true;
        // The view may have been resized while attaching.
        if (xterm.rows !== rows || xterm.cols !== cols) {
          resize();
        }
      },
    );
    attaching.catch((error) => setError(String(error)));

    xterm.onData((data) => {
      terminalInput(terminal, data).catch(console.error);
    });
    xterm.onResize(resize);
    const observer = new ResizeObserver(fitIfShown);
    observer.observe(element);

    return () => {
      observer.disconnect();
      xterm.dispose();
      // After the attachment ends, so a detach cannot overtake it.
      attaching
        .then(
          () => detachTerminal(terminal),
          () => undefined,
        )
        .catch(console.error);
    };
  }, [terminal]);

  return (
    <div className="terminal flex h-full min-h-0 flex-col p-3">
      {error !== undefined && <pre className="error m-0 whitespace-pre-wrap p-3 font-mono text-danger">{error}</pre>}
      <div ref={container} className="terminal-view min-h-0 flex-1" hidden={error !== undefined} />
    </div>
  );
}
