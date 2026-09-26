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
    <div ref={container} className="thread min-h-0 flex-1 overflow-y-auto p-5">
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
      return (
        <div className="block user mb-3 rounded border border-outline px-4 py-3 font-mono leading-relaxed whitespace-pre-wrap wrap-break-word last:mb-0">
          {item.text}
          {item.images.length > 0 && (
            <div className="thumbnails mt-2 flex flex-wrap gap-2">
              {item.images.map((image, index) => (
                <img key={index} className="max-h-[120px] max-w-40 rounded object-contain" src={`data:${image.mimeType};base64,${image.data}`} />
              ))}
            </div>
          )}
        </div>
      );
    case "agent":
      return <AgentMessage text={item.text} />;
    case "thought":
      return <Thought text={item.text} />;
    case "tool_call":
      return <ToolCall block={item} />;
    case "error":
      return (
        <div className="block error mb-3 flex gap-2 rounded border border-outline px-3 py-2 leading-relaxed text-danger last:mb-0">
          <span className="icon shrink-0">ⓘ</span>
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
    <div className="block thought mb-3 leading-relaxed last:mb-0">
      <div className="thought-row flex min-h-7 cursor-default items-center gap-2" onClick={() => setExpanded(!expanded)}>
        <span className="icon w-4 shrink-0 text-center text-fg-muted">{toolIcon("think")}</span>
        <span className="label">Thinking</span>
      </div>
      {expanded && <div className="thought-text ml-2 border-l border-outline pl-[15px] whitespace-pre-wrap text-fg-dim">{text}</div>}
    </div>
  );
}
