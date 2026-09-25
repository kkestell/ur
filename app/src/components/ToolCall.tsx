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
      <div className="block run-command">
        <div className="run-command-header" onClick={toggle}>
          <div className="run-command-label">Run Command</div>
          <div className="buffer">{block.title}</div>
        </div>
        {expanded && <ToolCallContentView content={block.content} />}
      </div>
    );
  }
  return (
    <div className="block tool-call">
      <div className="tool-call-row" onClick={toggle}>
        {/* A tool call without a tool kind is `other`, ACP's default. */}
        <span className="icon">{toolIcon(block.toolKind ?? "other")}</span>
        <span className="label">{block.title}</span>
      </div>
      {expanded && <ToolCallContentView content={block.content} />}
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
