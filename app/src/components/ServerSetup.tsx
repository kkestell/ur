import { ask, open } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { request } from "../ipc";
import type { ServerState } from "../ipc/bindings/ServerState";

export function ServerSetup({ servers, configError, initialServer, onSaved, onCancel }: {
  servers: ServerState[];
  configError: string | null;
  initialServer?: string;
  onSaved: () => void;
  onCancel: () => void;
}) {
  const [selected, setSelected] = useState<string | null>(initialServer ?? servers[0]?.id ?? null);
  const current = servers.find((server) => server.id === selected);
  const [name, setName] = useState(current?.name ?? "");
  const [command, setCommand] = useState(current?.command ?? "");
  const [args, setArgs] = useState<string[]>(current?.args ?? []);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [awaiting, setAwaiting] = useState<string | null>(null);

  useEffect(() => {
    setName(current?.name ?? "");
    setCommand(current?.command ?? "");
    setArgs(current?.args ?? []);
    setError(null);
  }, [selected, current?.id]);

  useEffect(() => {
    if (awaiting === null) return;
    const server = servers.find((server) => server.id === awaiting);
    if (server?.connected) onSaved();
    else if (server?.error) { setError(server.error); setAwaiting(null); }
  }, [awaiting, servers, onSaved]);

  async function choose() {
    try {
      const path = await open({ directory: false, multiple: false });
      if (typeof path === "string") setCommand(path);
    } catch (cause) { setError(String(cause)); }
  }

  async function save() {
    setSaving(true);
    setError(null);
    try {
      const response = await request(current
        ? { type: "update_server", server: current.id, name, command, args }
        : { type: "add_server", name, command, args });
      if (response.type === "error") { setError(response.message); return; }
      const id = response.type === "server_added" ? response.server : current?.id;
      if (id === undefined) return;
      setSelected(id);
      if (current?.connected && current.command === command && JSON.stringify(current.args) === JSON.stringify(args)) onSaved();
      else setAwaiting(id);
    } catch (cause) { setError(String(cause)); }
    finally { setSaving(false); }
  }

  async function remove() {
    if (!current) return;
    const confirmed = await ask(`Remove "${current.name}"? Its running sessions in ur will stop.`,
      { title: "Remove server?", kind: "warning", okLabel: "Remove" });
    if (!confirmed) return;
    const response = await request({ type: "remove_server", server: current.id });
    if (response.type === "error") { setError(response.message); return; }
    setAwaiting(null);
    setSelected(servers.find((server) => server.id !== current.id)?.id ?? null);
  }

  return (
    <div className="server-setup flex h-full items-center justify-center p-6">
      <div className="flex w-full max-w-2xl flex-col gap-4 text-sm">
        <h1 className="text-lg font-semibold">ACP servers</h1>
        <div className="flex flex-wrap gap-2">
          {servers.map((server) => <button key={server.id} className={`rounded px-3 py-1 ${selected === server.id ? "bg-control-hover" : "bg-control"}`} onClick={() => setSelected(server.id)}>
            {server.name} · {server.connected ? "Connected" : server.error ?? "Connecting"}
          </button>)}
          <button className="rounded bg-control px-3 py-1" onClick={() => setSelected(null)}>Add Server</button>
        </div>
        <label className="flex flex-col gap-1">Name
          <input className="server-name rounded bg-control px-2 py-1" value={name} onChange={(event) => setName(event.target.value)} />
        </label>
        <label className="flex flex-col gap-1">Executable
          <span className="flex gap-2">
            <input className="server-command min-w-0 flex-1 rounded bg-control px-2 py-1" value={command} onChange={(event) => setCommand(event.target.value)} />
            <button className="rounded bg-control px-3" onClick={() => void choose()}>Choose…</button>
          </span>
        </label>
        <div className="flex flex-col gap-2"><span>Arguments</span>
          {args.map((arg, index) => <span className="flex gap-2" key={index}>
            <input className="server-argument min-w-0 flex-1 rounded bg-control px-2 py-1" aria-label={`Argument ${index + 1}`} value={arg} onChange={(event) => setArgs(args.map((value, i) => i === index ? event.target.value : value))} />
            <button className="rounded bg-control px-3" aria-label={`Remove argument ${index + 1}`} onClick={() => setArgs(args.filter((_, i) => i !== index))}>Remove</button>
          </span>)}
          <button className="self-start rounded bg-control px-3 py-1" onClick={() => setArgs([...args, ""])}>Add argument</button>
        </div>
        {(error ?? current?.error ?? configError) && <p className="server-error text-red-400">{error ?? current?.error ?? configError}</p>}
        {awaiting && !error && <p className="text-fg-dim">Connecting to server…</p>}
        <div className="flex gap-2">
          <button className="server-save rounded bg-control px-3 py-1" disabled={saving || !name.trim() || !command} onClick={() => void save()}>{saving ? "Saving…" : "Save"}</button>
          {current && <button className="server-remove rounded bg-control px-3 py-1" onClick={() => void remove()}>Remove</button>}
          <button className="rounded bg-control px-3 py-1" onClick={onCancel}>Close</button>
        </div>
      </div>
    </div>
  );
}
