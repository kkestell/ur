import type { ToolCallContent, ToolCallStatus, ToolKind } from "@agentclientprotocol/sdk";

/** What the thread renders. */
export type Block =
  | { kind: "user"; text: string }
  | { kind: "agent"; text: string }
  | { kind: "thought"; text: string }
  | {
      kind: "tool_call";
      id: string;
      title: string;
      toolKind?: ToolKind;
      status?: ToolCallStatus;
      content: ToolCallContent[];
    }
  | { kind: "error"; message: string };

export type ThreadState = { blocks: Block[] };
