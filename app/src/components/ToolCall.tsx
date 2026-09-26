import type { ToolKind } from "@agentclientprotocol/sdk";
import { useState } from "react";
import type { Block } from "../transcript/blocks";
import { ToolCallContentView } from "./ToolCallContent";

/**
 * A tool call: a Run Command block for tool kind `execute`, and otherwise a
 * row with the glyph for its tool kind. Clicking the row or the command shows
 * the tool call content beneath it, and clicking again hides it.
 */
export function ToolCall({ block }: { block: Extract<Block, { kind: "tool_call" }> }) {
  const [expanded, setExpanded] = useState(false);
  const toggle = () => setExpanded(!expanded);
  if (block.toolKind === "execute") {
    return (
      <div className="block run-command mb-3 overflow-hidden rounded bg-raised leading-relaxed last:mb-0">
        <div className="run-command-header cursor-default px-4 py-3" onClick={toggle}>
          <div className="run-command-label font-mono text-label text-fg-dim">Run Command</div>
          <div className="buffer">{block.title}</div>
        </div>
        {expanded && <div className="space-y-2 border-t border-outline px-4 py-3 text-fg-muted"><ToolCallContentView content={block.content} /></div>}
      </div>
    );
  }
  return (
    <div className="block tool-call mb-3 leading-relaxed last:mb-0">
      <div className="tool-call-row flex min-h-7 cursor-default items-center gap-2" onClick={toggle}>
        {/* A tool call without a tool kind is `other`, ACP's default. */}
        <span className="icon w-4 shrink-0 text-center text-fg-muted">{toolIcon(block.toolKind ?? "other")}</span>
        <span className="label min-w-0 truncate">{block.title}</span>
      </div>
      {expanded && <div className="mt-2 space-y-2 rounded border border-outline p-3 text-fg-dim"><ToolCallContentView content={block.content} /></div>}
    </div>
  );
}

/** The glyph for a tool kind. `execute` has none: it is a Run Command block. */
export function toolIcon(kind: Exclude<ToolKind, "execute">): string {
  switch (kind) {
    case "read":
    case "search":
      return "⌕";
    case "edit":
      return "✎";
    case "delete":
      return "✕";
    case "move":
      return "⇢";
    case "fetch":
      return "⇣";
    case "think":
      return "✧";
    case "switch_mode":
      return "⇄";
    case "other":
      return "•";
  }
}
