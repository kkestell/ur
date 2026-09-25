import type { PendingPermission } from "../ipc/bindings/PendingPermission";
import type { Block } from "./blocks";

/** A block, or a pending permission request rendered in the thread. */
export type Item = Block | { kind: "permission"; request: PendingPermission; title: string };

/**
 * Places the session's pending permission requests, oldest first, among its
 * blocks. A request takes the place of the tool call block with its tool
 * call ID, keeping the block's title when the request supplies none. A
 * request whose tool call has no block yet follows the last item, titled by
 * its tool call ID when it supplies no title.
 */
export function withPermissions(blocks: Block[], requests: PendingPermission[]): Item[] {
  const items: Item[] = [...blocks];
  for (const request of requests) {
    const toolCall = request.request.toolCall;
    const title = typeof toolCall.title === "string" ? toolCall.title : undefined;
    const index = items.findIndex(
      (item) => item.kind === "tool_call" && item.id === toolCall.toolCallId,
    );
    const block = items[index];
    if (block !== undefined && block.kind === "tool_call") {
      items[index] = { kind: "permission", request, title: title ?? block.title };
    } else {
      items.push({ kind: "permission", request, title: title ?? toolCall.toolCallId });
    }
  }
  return items;
}
