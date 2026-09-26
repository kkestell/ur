import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { request } from "../ipc";
import type { ServerState } from "../ipc/bindings/ServerState";

export function ServerSetup({
  state,
  onSaved,
  onCancel,
}: {
  state: ServerState;
  onSaved: () => void;
  onCancel?: () => void;
}) {
  const [command, setCommand] = useState(state.command ?? "");
  const [args, setArgs] = useState<string[]>(state.args);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [awaitingConnection, setAwaitingConnection] = useState(false);

  useEffect(() => {
    setCommand(state.command ?? "");
    setArgs(state.args);
  }, [state.command, state.args]);

  useEffect(() => {
    if (awaitingConnection && state.connected) onSaved();
  }, [awaitingConnection, state.connected, onSaved]);

  async function choose() {
    try {
      const path = await open({ directory: false, multiple: false });
      if (typeof path === "string") setCommand(path);
    } catch (error) {
      setError(String(error));
    }
  }

  async function save() {
    setSaving(true);
    setError(null);
    try {
      const response = await request({ type: "set_server", command, args });
      if (response.type === "error") {
        setError(response.message);
      } else {
        if (state.connected) onSaved();
        else setAwaitingConnection(true);
      }
    } catch (error) {
      setError(String(error));
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="server-setup flex h-full items-center justify-center p-6">
      <div className="flex w-full max-w-lg flex-col gap-4 text-sm">
        <h1 className="text-lg font-semibold">Connect an ACP server</h1>
        <p className="text-fg-dim">Choose the server executable installed on this Mac, then add its arguments.</p>
        <label className="flex flex-col gap-1">
          Executable
          <span className="flex gap-2">
            <input className="server-command min-w-0 flex-1 rounded bg-control px-2 py-1" value={command} onChange={(event) => setCommand(event.target.value)} />
            <button className="rounded bg-control px-3" onClick={() => void choose()}>Choose…</button>
          </span>
        </label>
        <div className="flex flex-col gap-2">
          <span>Arguments</span>
          {args.map((arg, index) => (
            <span className="flex gap-2" key={index}>
              <input className="server-argument min-w-0 flex-1 rounded bg-control px-2 py-1" aria-label={`Argument ${index + 1}`} value={arg} onChange={(event) => setArgs(args.map((value, i) => i === index ? event.target.value : value))} />
              <button className="rounded bg-control px-3" aria-label={`Remove argument ${index + 1}`} onClick={() => setArgs(args.filter((_, i) => i !== index))}>Remove</button>
            </span>
          ))}
          <button className="self-start rounded bg-control px-3 py-1" onClick={() => setArgs([...args, ""])}>Add argument</button>
        </div>
        {(error ?? state.error) && <p className="server-error text-red-400">{error ?? state.error}</p>}
        {awaitingConnection && !state.connected && !state.error && <p className="text-fg-dim">Connecting to server…</p>}
        <div className="flex gap-2">
          <button className="server-save rounded bg-control px-3 py-1" disabled={saving || command.length === 0} onClick={() => void save()}>{saving ? "Saving…" : "Connect"}</button>
          {onCancel && <button className="rounded bg-control px-3 py-1" onClick={onCancel}>Cancel</button>}
        </div>
      </div>
    </div>
  );
}
