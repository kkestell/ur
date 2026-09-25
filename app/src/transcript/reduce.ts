import type { ContentBlock, SessionUpdate } from "@agentclientprotocol/sdk";
import type { SessionEvent } from "../ipc";
import type { Entry } from "../ipc/bindings/Entry";
import type { Block, ThreadState } from "./blocks";

/**
 * The transcript reducer. A snapshot builds fresh blocks, which is what keeps
 * replay after a reconnect or a load free of duplicates; an entry appends
 * one; `session_removed` yields `undefined`, and the store drops the session.
 */
export function reduce(state: ThreadState, event: SessionEvent): ThreadState | undefined {
  switch (event.type) {
    case "session_snapshot": {
      const blocks: Block[] = [];
      for (const entry of event.transcript) {
        applyEntry(blocks, entry);
      }
      return { blocks };
    }
    case "entry": {
      const blocks = [...state.blocks];
      applyEntry(blocks, event.entry);
      return { blocks };
    }
    case "session_removed":
      return undefined;
  }
}

/** Appends the entry to `blocks`, or updates the block it changes. */
export function applyEntry(blocks: Block[], entry: Entry): void {
  switch (entry.type) {
    case "user_prompt":
      blocks.push({ kind: "user", text: text(entry.content) });
      return;
    case "turn_error":
      blocks.push({ kind: "error", message: entry.message });
      return;
    case "update":
      applyUpdate(blocks, entry.update);
      return;
  }
}

function applyUpdate(blocks: Block[], update: SessionUpdate): void {
  switch (update.sessionUpdate) {
    case "user_message_chunk":
      appendText(blocks, "user", update.content);
      return;
    case "agent_message_chunk":
      appendText(blocks, "agent", update.content);
      return;
    case "agent_thought_chunk":
      appendText(blocks, "thought", update.content);
      return;
    case "tool_call":
      blocks.push({
        kind: "tool_call",
        id: update.toolCallId,
        title: update.title,
        toolKind: update.kind,
        status: update.status,
        content: update.content ?? [],
      });
      return;
    case "tool_call_update": {
      const index = blocks.findIndex(
        (block) => block.kind === "tool_call" && block.id === update.toolCallId,
      );
      const block = blocks[index];
      if (block === undefined || block.kind !== "tool_call") {
        blocks.push({
          kind: "tool_call",
          id: update.toolCallId,
          title: update.title ?? update.toolCallId,
          toolKind: update.kind ?? undefined,
          status: update.status ?? undefined,
          content: update.content ?? [],
        });
        return;
      }
      blocks[index] = {
        ...block,
        title: update.title ?? block.title,
        toolKind: update.kind ?? block.toolKind,
        status: update.status ?? block.status,
        // ACP replaces the whole list; `null` or no `content` leaves it.
        content: update.content ?? block.content,
      };
      return;
    }
    default:
      // Plans, commands, config options, session info, usage, modes, and the
      // rest are not shown here yet.
      return;
  }
}

/**
 * Appends the chunk's text to the last block when it is of the same kind, and
 * otherwise starts a new block. Only text content is read.
 */
function appendText(blocks: Block[], kind: "user" | "agent" | "thought", content: ContentBlock) {
  if (content.type !== "text") {
    return;
  }
  const last = blocks[blocks.length - 1];
  if (last !== undefined && last.kind === kind) {
    blocks[blocks.length - 1] = { kind, text: last.text + content.text };
  } else {
    blocks.push({ kind, text: content.text });
  }
}

/** The text parts joined by newlines. */
function text(content: ContentBlock[]): string {
  return content
    .filter((block) => block.type === "text")
    .map((block) => block.text)
    .join("\n");
}
