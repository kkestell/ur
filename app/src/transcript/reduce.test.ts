import type { SessionUpdate } from "@agentclientprotocol/sdk";
import { expect, test } from "vitest";
import type { Entry } from "../ipc/bindings/Entry";
import type { Block, ThreadState } from "./blocks";
import { reduce } from "./reduce";

const SESSION = "s1";

function update(update: SessionUpdate): Entry {
  return { type: "update", update };
}

function chunk(
  sessionUpdate: "user_message_chunk" | "agent_message_chunk" | "agent_thought_chunk",
  text: string,
): Entry {
  return update({ sessionUpdate, content: { type: "text", text } });
}

function reduceAll(entries: Entry[], state: ThreadState = { blocks: [] }): Block[] {
  let current: ThreadState | undefined = state;
  for (const entry of entries) {
    current = reduce(current!, { type: "entry", session: SESSION, entry });
  }
  return current!.blocks;
}

test("a_snapshot_replaces_the_thread", () => {
  const transcript: Entry[] = [
    { type: "user_prompt", content: [{ type: "text", text: "hi" }] },
    chunk("agent_message_chunk", "hello"),
  ];
  const once = reduce({ blocks: [] }, { type: "session_snapshot", session: SESSION, transcript });
  const again = reduce(once!, { type: "session_snapshot", session: SESSION, transcript });
  expect(again!.blocks).toEqual([
    { kind: "user", text: "hi" },
    { kind: "agent", text: "hello" },
  ]);
});

test.each([
  {
    name: "agent message",
    entries: [chunk("agent_message_chunk", "one "), chunk("agent_message_chunk", "two")],
    blocks: [{ kind: "agent", text: "one two" }],
  },
  {
    name: "thought",
    entries: [chunk("agent_thought_chunk", "hm"), chunk("agent_thought_chunk", "m")],
    blocks: [{ kind: "thought", text: "hmm" }],
  },
  {
    name: "replayed user message",
    entries: [chunk("user_message_chunk", "do "), chunk("user_message_chunk", "it")],
    blocks: [{ kind: "user", text: "do it" }],
  },
  {
    name: "a kind change starts a new block",
    entries: [chunk("agent_thought_chunk", "hm"), chunk("agent_message_chunk", "ok")],
    blocks: [
      { kind: "thought", text: "hm" },
      { kind: "agent", text: "ok" },
    ],
  },
])("consecutive_chunks_form_one_block: $name", ({ entries, blocks }) => {
  expect(reduceAll(entries)).toEqual(blocks);
});

test("a_user_prompt_joins_its_text_parts", () => {
  const entry: Entry = {
    type: "user_prompt",
    content: [
      { type: "text", text: "first" },
      { type: "image", data: "", mimeType: "image/png" },
      { type: "text", text: "second" },
    ],
  };
  expect(reduceAll([entry])).toEqual([{ kind: "user", text: "first\nsecond" }]);
});

const call: Entry = update({
  sessionUpdate: "tool_call",
  toolCallId: "t1",
  title: "Read a.rs",
  kind: "read",
  status: "pending",
});

test.each([
  {
    name: "a status change",
    entries: [
      call,
      update({ sessionUpdate: "tool_call_update", toolCallId: "t1", status: "completed" }),
    ],
    blocks: [
      { kind: "tool_call", id: "t1", title: "Read a.rs", toolKind: "read", status: "completed" },
    ],
  },
  {
    name: "a title change",
    entries: [
      call,
      update({ sessionUpdate: "tool_call_update", toolCallId: "t1", title: "Read b.rs" }),
    ],
    blocks: [
      { kind: "tool_call", id: "t1", title: "Read b.rs", toolKind: "read", status: "pending" },
    ],
  },
  {
    name: "a null field left unchanged",
    entries: [
      call,
      update({
        sessionUpdate: "tool_call_update",
        toolCallId: "t1",
        title: null,
        kind: null,
        status: "in_progress",
      }),
    ],
    blocks: [
      { kind: "tool_call", id: "t1", title: "Read a.rs", toolKind: "read", status: "in_progress" },
    ],
  },
  {
    name: "an unknown ID appending a block",
    entries: [
      call,
      update({ sessionUpdate: "tool_call_update", toolCallId: "t2", status: "completed" }),
    ],
    blocks: [
      { kind: "tool_call", id: "t1", title: "Read a.rs", toolKind: "read", status: "pending" },
      { kind: "tool_call", id: "t2", title: "t2", toolKind: undefined, status: "completed" },
    ],
  },
])("tool_call_updates_merge_by_id: $name", ({ entries, blocks }) => {
  expect(reduceAll(entries)).toEqual(blocks);
});

test("a_turn_error_is_an_error_block", () => {
  expect(reduceAll([{ type: "turn_error", message: "boom" }])).toEqual([
    { kind: "error", message: "boom" },
  ]);
});

test.each([
  {
    name: "session_info_update",
    entry: update({ sessionUpdate: "session_info_update", title: "Renamed" }),
  },
  {
    name: "available_commands_update",
    entry: update({ sessionUpdate: "available_commands_update", availableCommands: [] }),
  },
])("other_updates_are_ignored: $name", ({ entry }) => {
  expect(reduceAll([chunk("agent_message_chunk", "hi"), entry])).toEqual([
    { kind: "agent", text: "hi" },
  ]);
});
