import type { PermissionOption } from "@agentclientprotocol/sdk";
import { useEffect, useRef } from "react";
import type { PendingPermission } from "../ipc/bindings/PendingPermission";
import type { Item } from "../transcript/permissions";
import { Permission } from "./Permission";

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
      return <div className="block agent">{item.text}</div>;
    case "thought":
      return <div className="block thought">{item.text}</div>;
    case "tool_call":
      return <div className="block tool-call">{item.title}</div>;
    case "error":
      return <div className="block error">{item.message}</div>;
    case "permission":
      return (
        <Permission
          title={item.title}
          request={item.request}
          onAnswer={(option) => onAnswer(item.request, option)}
        />
      );
  }
}
