import { useEffect, useRef, useState } from "react";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { attachTerminal, detachTerminal, request, terminalInput } from "../ipc";
import type { TerminalSummary } from "../ipc/bindings/TerminalSummary";

/**
 * The terminal header above the terminal's xterm.js view. Attaches on mount
 * and detaches on unmount, so render it keyed by terminal ID.
 */
export function TerminalPane({ summary }: { summary: TerminalSummary }) {
  const terminal = summary.terminal;
  const container = useRef<HTMLDivElement>(null);
  const [error, setError] = useState<string>();

  useEffect(() => {
    const xterm = new Terminal({ fontFamily: "Menlo, monospace", fontSize: 13 });
    const fit = new FitAddon();
    xterm.loadAddon(fit);
    xterm.open(container.current!);
    fit.fit();

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
    const observer = new ResizeObserver(() => fit.fit());
    observer.observe(container.current!);

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
    <div className="terminal">
      <div className="terminal-header">
        <span className="terminal-icon">&gt;_</span>
        <span className="label">{summary.title}</span>
      </div>
      {error !== undefined && <pre className="error">{error}</pre>}
      <div ref={container} className="terminal-view" hidden={error !== undefined} />
    </div>
  );
}
