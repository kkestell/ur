import type { PermissionOption, ToolCallContent } from "@agentclientprotocol/sdk";
import type { PendingPermission } from "../ipc/bindings/PendingPermission";
import { shortcutLabel } from "../keys";
import { ToolCallContentView } from "./ToolCallContent";

/** One pending permission request: its tool call, its options, and "Awaiting Confirmation." */
export function Permission({
  title,
  content,
  request,
  onAnswer,
}: {
  title: string;
  content: ToolCallContent[];
  request: PendingPermission;
  onAnswer: (option: PermissionOption) => void;
}) {
  const { options } = request.request;
  // A request with two options of the same kind shows the shortcut on the first.
  const shown = new Set<string>();
  return (
    <>
      <div className="block permission mb-2 rounded border border-outline px-4 py-3 leading-relaxed">
        <div className="permission-title mb-2">{title}</div>
        <div className="mb-2 space-y-2 rounded bg-raised p-2 text-fg-muted"><ToolCallContentView content={content} /></div>
        <div className="permission-options flex flex-col">
          {options.map((option) => {
            const first = !shown.has(option.kind);
            shown.add(option.kind);
            return (
              <button key={option.optionId} className="option flex min-h-7 items-center gap-2 rounded px-2 py-1 text-left hover:bg-control" onClick={() => onAnswer(option)}>
                <span className="icon w-4 shrink-0 text-center text-fg-muted">{icon(option.kind)}</span>
                <span className="label min-w-0 flex-1">{option.name}</span>
                {first && <span className="shortcut text-fg-dim">{shortcutLabel(option.kind)}</span>}
              </button>
            );
          })}
        </div>
      </div>
      <div className="block awaiting mb-3 text-fg-dim last:mb-0">Awaiting Confirmation.</div>
    </>
  );
}

function icon(kind: PermissionOption["kind"]): string {
  switch (kind) {
    case "allow_once":
    case "allow_always":
      return "✓";
    case "reject_once":
    case "reject_always":
      return "✕";
  }
}
