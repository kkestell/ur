import type { SessionConfigOption, SessionUpdate, ToolCallContent } from "@agentclientprotocol/sdk";
import { expect, test } from "vitest";
import type { Entry } from "../ipc/bindings/Entry";
import { type Block, type ThreadState, emptyThread } from "./blocks";
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

function reduceThread(entries: Entry[], state: ThreadState = emptyThread): ThreadState {
  let current: ThreadState | undefined = state;
  for (const entry of entries) {
    current = reduce(current!, { type: "entry", session: SESSION, entry });
  }
  return current!;
}

function reduceAll(entries: Entry[]): Block[] {
  return reduceThread(entries).blocks;
}

function snapshot(transcript: Entry[], config_options: SessionConfigOption[] = []): ThreadState {
  return reduce(emptyThread, {
    type: "session_snapshot",
    session: SESSION,
    transcript,
    config_options,
  })!;
}

test("a_snapshot_replaces_the_thread", () => {
  const transcript: Entry[] = [
    { type: "user_prompt", content: [{ type: "text", text: "hi" }] },
    chunk("agent_message_chunk", "hello"),
  ];
  const once = snapshot(transcript);
  const again = reduce(once, {
    type: "session_snapshot",
    session: SESSION,
    transcript,
    config_options: [],
  });
  expect(again!.blocks).toEqual([
    { kind: "user", text: "hi", images: [] },
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
    blocks: [{ kind: "user", text: "do it", images: [] }],
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
      { type: "text", text: "second" },
    ],
  };
  expect(reduceAll([entry])).toEqual([{ kind: "user", text: "first\nsecond", images: [] }]);
});

const png = { mimeType: "image/png", data: "iVBOR" };

test.each([
  {
    name: "a user prompt entry",
    entries: [
      {
        type: "user_prompt",
        content: [
          { type: "text", text: "look" },
          { type: "image", ...png },
        ],
      } satisfies Entry,
    ],
  },
  {
    name: "replayed user_message_chunk updates",
    entries: [
      chunk("user_message_chunk", "look"),
      update({ sessionUpdate: "user_message_chunk", content: { type: "image", ...png } }),
    ],
  },
])("a_user_message_keeps_its_images: $name", ({ entries }) => {
  expect(reduceAll(entries)).toEqual([{ kind: "user", text: "look", images: [png] }]);
});

const call: Entry = update({
  sessionUpdate: "tool_call",
  toolCallId: "t1",
  title: "Read a.rs",
  kind: "read",
  status: "pending",
});

function output(text: string): ToolCallContent {
  return { type: "content", content: { type: "text", text } };
}

const callWithOutput: Entry = update({
  sessionUpdate: "tool_call",
  toolCallId: "t1",
  title: "Read a.rs",
  kind: "read",
  status: "pending",
  content: [output("one")],
});

test.each([
  {
    name: "a status change",
    entries: [
      call,
      update({ sessionUpdate: "tool_call_update", toolCallId: "t1", status: "completed" }),
    ],
    blocks: [
      {
        kind: "tool_call",
        id: "t1",
        title: "Read a.rs",
        toolKind: "read",
        status: "completed",
        content: [],
      },
    ],
  },
  {
    name: "a title change",
    entries: [
      call,
      update({ sessionUpdate: "tool_call_update", toolCallId: "t1", title: "Read b.rs" }),
    ],
    blocks: [
      {
        kind: "tool_call",
        id: "t1",
        title: "Read b.rs",
        toolKind: "read",
        status: "pending",
        content: [],
      },
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
      {
        kind: "tool_call",
        id: "t1",
        title: "Read a.rs",
        toolKind: "read",
        status: "in_progress",
        content: [],
      },
    ],
  },
  {
    name: "content replaced",
    entries: [
      callWithOutput,
      update({
        sessionUpdate: "tool_call_update",
        toolCallId: "t1",
        content: [output("two"), output("three")],
      }),
    ],
    blocks: [
      {
        kind: "tool_call",
        id: "t1",
        title: "Read a.rs",
        toolKind: "read",
        status: "pending",
        content: [output("two"), output("three")],
      },
    ],
  },
  {
    name: "a null content left unchanged",
    entries: [
      callWithOutput,
      update({ sessionUpdate: "tool_call_update", toolCallId: "t1", content: null }),
    ],
    blocks: [
      {
        kind: "tool_call",
        id: "t1",
        title: "Read a.rs",
        toolKind: "read",
        status: "pending",
        content: [output("one")],
      },
    ],
  },
  {
    name: "an unknown ID appending a block",
    entries: [
      call,
      update({ sessionUpdate: "tool_call_update", toolCallId: "t2", status: "completed" }),
    ],
    blocks: [
      {
        kind: "tool_call",
        id: "t1",
        title: "Read a.rs",
        toolKind: "read",
        status: "pending",
        content: [],
      },
      {
        kind: "tool_call",
        id: "t2",
        title: "t2",
        toolKind: undefined,
        status: "completed",
        content: [],
      },
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
    name: "config_option_update",
    entry: update({ sessionUpdate: "config_option_update", configOptions: [] }),
  },
])("other_updates_are_ignored: $name", ({ entry }) => {
  expect(reduceAll([chunk("agent_message_chunk", "hi"), entry])).toEqual([
    { kind: "agent", text: "hi" },
  ]);
});

function commands(...names: string[]): Entry {
  return update({
    sessionUpdate: "available_commands_update",
    availableCommands: names.map((name) => ({ name, description: `run ${name}` })),
  });
}

function usage(used: number): Entry {
  return update({ sessionUpdate: "usage_update", used, size: 8000 });
}

test.each([
  {
    name: "from entries",
    thread: () => reduceThread([commands("one"), usage(10), commands("two"), usage(20)]),
  },
  {
    name: "from the snapshot",
    thread: () => snapshot([commands("one"), usage(10), commands("two"), usage(20)]),
  },
])("the_latest_commands_and_usage_are_kept: $name", ({ thread }) => {
  const state = thread();
  expect(state.commands.map((command) => command.name)).toEqual(["two"]);
  expect(state.usage).toMatchObject({ used: 20, size: 8000 });
});

function pace(currentValue: string): SessionConfigOption[] {
  return [
    {
      type: "select",
      id: "pace",
      name: "Pace",
      currentValue,
      options: [
        { value: "steady", name: "Steady" },
        { value: "brisk", name: "Brisk" },
      ],
    },
  ];
}

test.each([
  { name: "the snapshot", thread: () => snapshot([], pace("brisk")) },
  {
    name: "config_options_changed",
    thread: () =>
      reduce(snapshot([], pace("steady")), {
        type: "config_options_changed",
        session: SESSION,
        config_options: pace("brisk"),
      })!,
  },
])("config_options_come_from: $name", ({ thread }) => {
  expect(thread().configOptions).toEqual(pace("brisk"));
});
