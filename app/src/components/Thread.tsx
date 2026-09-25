import type { PermissionOption } from "@agentclientprotocol/sdk";
import { useEffect, useRef, useState } from "react";
import type { PendingPermission } from "../ipc/bindings/PendingPermission";
import type { Item } from "../transcript/permissions";
import { AgentMessage } from "./AgentMessage";
import { Permission } from "./Permission";
import { ToolCall, toolIcon } from "./ToolCall";

export function Thread({
  items,
  onAnswer,
}: {
  items: Item[];
  onAnswer: (request: PendingPermission, option: PermissionOption) => void;
}) {
  const container = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const element = container.current!;
    element.scrollTop = element.scrollHeight;
  }, [items]);

  return (
    <div ref={container} className="thread">
      {items.map((item, index) => (
        <ItemView key={index} item={item} onAnswer={onAnswer} />
      ))}
    </div>
  );
}

function ItemView({
  item,
  onAnswer,
}: {
  item: Item;
  onAnswer: (request: PendingPermission, option: PermissionOption) => void;
}) {
  switch (item.kind) {
    case "user":
      return <div className="block user">{item.text}</div>;
    case "agent":
      return <AgentMessage text={item.text} />;
    case "thought":
      return <Thought text={item.text} />;
    case "tool_call":
      return <ToolCall block={item} />;
    case "error":
      return (
        <div className="block error">
          <span className="icon">ⓘ</span>
          <span>{item.message}</span>
        </div>
      );
    case "permission":
      return (
        <Permission
          title={item.title}
          content={item.content}
          request={item.request}
          onAnswer={(option) => onAnswer(item.request, option)}
        />
      );
  }
}

/**
 * A collapsed "Thinking" row. Clicking it shows the thought's text, and
 * clicking again hides it.
 */
function Thought({ text }: { text: string }) {
  const [expanded, setExpanded] = useState(false);
  return (
    <div className="block thought">
      <div className="thought-row" onClick={() => setExpanded(!expanded)}>
        <span className="icon">{toolIcon("think")}</span>
        <span className="label">Thinking</span>
      </div>
      {expanded && <div className="thought-text">{text}</div>}
    </div>
  );
}
