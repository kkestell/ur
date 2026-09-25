import { useEffect, useRef } from "react";
import type { Block } from "../transcript/blocks";

export function Thread({ blocks }: { blocks: Block[] }) {
  const container = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const element = container.current!;
    element.scrollTop = element.scrollHeight;
  }, [blocks]);

  return (
    <div ref={container} className="thread">
      {blocks.map((block, index) => (
        <BlockView key={index} block={block} />
      ))}
    </div>
  );
}

function BlockView({ block }: { block: Block }) {
  switch (block.kind) {
    case "user":
      return <div className="block user">{block.text}</div>;
    case "agent":
      return <div className="block agent">{block.text}</div>;
    case "thought":
      return <div className="block thought">{block.text}</div>;
    case "tool_call":
      return <div className="block tool-call">{block.title}</div>;
    case "error":
      return <div className="block error">{block.message}</div>;
  }
}
