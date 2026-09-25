import { useEffect, useRef } from "react";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { attachTerminal, request, terminalInput } from "../ipc";

export function TerminalPane({ onError }: { onError: (error: string) => void }) {
  const container = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const xterm = new Terminal({ fontFamily: "Menlo, monospace", fontSize: 13 });
    const fit = new FitAddon();
    xterm.loadAddon(fit);
    xterm.open(container.current!);
    fit.fit();

    let terminal: number | undefined;
    const resize = () => {
      if (terminal !== undefined) {
        const { rows, cols } = xterm;
        request({ type: "terminal_resize", terminal, rows, cols }).catch(console.error);
      }
    };
    const { rows, cols } = xterm;
    attachTerminal(rows, cols, (bytes) => xterm.write(bytes))
      .then((id) => {
        terminal = id;
        // The view may have been resized while attaching.
        if (xterm.rows !== rows || xterm.cols !== cols) {
          resize();
        }
      })
      .catch((error) => onError(String(error)));

    xterm.onData((data) => {
      if (terminal !== undefined) {
        terminalInput(terminal, data).catch(console.error);
      }
    });
    xterm.onResize(resize);
    const observer = new ResizeObserver(() => fit.fit());
    observer.observe(container.current!);

    return () => {
      observer.disconnect();
      xterm.dispose();
    };
  }, [onError]);

  return <div ref={container} style={{ height: "100%" }} />;
}
