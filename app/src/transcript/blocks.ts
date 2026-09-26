import type {
  AvailableCommand,
  SessionConfigOption,
  ToolCallContent,
  ToolCallStatus,
  ToolKind,
  UsageUpdate,
} from "@agentclientprotocol/sdk";

/** An image part of a user message. */
export type UserImage = { mimeType: string; data: string };

/** What the thread renders. */
export type Block =
  | { kind: "user"; text: string; images: UserImage[] }
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

/**
 * One session's thread: its blocks, the slash commands from the latest
 * `available_commands_update`, the latest `usage_update`, and its config
 * options.
 */
export type ThreadState = {
  blocks: Block[];
  commands: AvailableCommand[];
  usage: UsageUpdate | null;
  configOptions: SessionConfigOption[];
};

export const emptyThread: ThreadState = {
  blocks: [],
  commands: [],
  usage: null,
  configOptions: [],
};
