import type { PermissionOption } from "@agentclientprotocol/sdk";
import { expect, test } from "vitest";
import type { PendingPermission } from "../ipc/bindings/PendingPermission";
import type { Block } from "./blocks";
import { withPermissions } from "./permissions";

const options: PermissionOption[] = [{ optionId: "go", name: "Go ahead", kind: "allow_once" }];

function pending(request_id: number, toolCallId: string, title?: string): PendingPermission {
  return {
    request_id,
    request: { sessionId: "s", toolCall: { toolCallId, title }, options },
  };
}

const blocks: Block[] = [
  { kind: "user", text: "count" },
  { kind: "tool_call", id: "t1", title: "count the tallies" },
  { kind: "agent", text: "counting" },
];

test("a_request_takes_its_tool_call_rows_place", () => {
  const untitled = withPermissions(blocks, [pending(1, "t1")]);
  expect(untitled.map((item) => item.kind)).toEqual(["user", "permission", "agent"]);
  expect(untitled[1]).toMatchObject({ kind: "permission", title: "count the tallies" });

  const titled = withPermissions(blocks, [pending(1, "t1", "read the tallies")]);
  expect(titled[1]).toMatchObject({ kind: "permission", title: "read the tallies" });
});

test("a_request_without_a_tool_call_row_follows_the_last_entry", () => {
  const items = withPermissions(blocks, [pending(1, "t9")]);
  expect(items.map((item) => item.kind)).toEqual(["user", "tool_call", "agent", "permission"]);
  expect(items[3]).toMatchObject({ kind: "permission", title: "t9" });
});

test("requests_keep_their_order", () => {
  const items = withPermissions(blocks, [pending(1, "t1"), pending(2, "t9")]);
  expect(items.map((item) => item.kind)).toEqual(["user", "permission", "agent", "permission"]);
  expect(items[1]).toMatchObject({ request: { request_id: 1 } });
  expect(items[3]).toMatchObject({ request: { request_id: 2 } });
});
