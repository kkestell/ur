import type { ContentBlock, SessionUpdate } from "@agentclientprotocol/sdk";
import type { SessionEvent } from "../ipc";
import type { Entry } from "../ipc/bindings/Entry";
import { type Block, type ThreadState, type UserImage, emptyThread } from "./blocks";

/**
 * The transcript reducer. A snapshot builds a fresh thread, which is what
 * keeps replay after a reconnect or a load free of duplicates; an entry
 * appends one; `config_options_changed` replaces the config options;
 * `session_removed` yields `undefined`, and the store drops the session.
 */
export function reduce(state: ThreadState, event: SessionEvent): ThreadState | undefined {
  switch (event.type) {
    case "session_snapshot": {
      const thread: ThreadState = {
        ...emptyThread,
        blocks: [],
        configOptions: event.config_options,
      };
      for (const entry of event.transcript) {
        applyEntry(thread, entry);
      }
      return thread;
    }
    case "entry": {
      const thread = { ...state, blocks: [...state.blocks] };
      applyEntry(thread, event.entry);
      return thread;
    }
    case "config_options_changed":
      return { ...state, configOptions: event.config_options };
    case "session_removed":
      return undefined;
  }
}

/**
 * Appends the entry to the thread's blocks, or updates the block it changes.
 * A slash command or usage update replaces the thread's commands or usage.
 */
export function applyEntry(thread: ThreadState, entry: Entry): void {
  const blocks = thread.blocks;
  switch (entry.type) {
    case "user_prompt":
      blocks.push({ kind: "user", text: text(entry.content), images: images(entry.content) });
      return;
    case "turn_error":
      blocks.push({ kind: "error", message: entry.message });
      return;
    case "update":
      applyUpdate(thread, entry.update);
      return;
  }
}

function applyUpdate(thread: ThreadState, update: SessionUpdate): void {
  const blocks = thread.blocks;
  switch (update.sessionUpdate) {
    case "user_message_chunk":
      appendUserPart(blocks, update.content);
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
    case "available_commands_update":
      thread.commands = update.availableCommands;
      return;
    case "usage_update":
      thread.usage = update;
      return;
    default:
      // Config options come from the snapshot and `config_options_changed`.
      // Plans, session info, modes, and the rest are not shown here yet.
      return;
  }
}

/**
 * Adds a replayed user message chunk's text or image to the last block when
 * it is a user message, and otherwise starts a new one.
 */
function appendUserPart(blocks: Block[], content: ContentBlock) {
  const last = blocks[blocks.length - 1];
  const user = last !== undefined && last.kind === "user" ? last : undefined;
  const text = content.type === "text" ? content.text : "";
  const image = images([content]);
  if (text === "" && image.length === 0) {
    return;
  }
  const block: Block = {
    kind: "user",
    text: (user?.text ?? "") + text,
    images: [...(user?.images ?? []), ...image],
  };
  if (user !== undefined) {
    blocks[blocks.length - 1] = block;
  } else {
    blocks.push(block);
  }
}

/**
 * Appends the chunk's text to the last block when it is of the same kind, and
 * otherwise starts a new block. Only text content is read.
 */
function appendText(blocks: Block[], kind: "agent" | "thought", content: ContentBlock) {
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

/** The image parts. */
function images(content: ContentBlock[]): UserImage[] {
  return content.flatMap((block) =>
    block.type === "image" ? [{ mimeType: block.mimeType, data: block.data }] : [],
  );
}
