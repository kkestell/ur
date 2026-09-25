# Thread rendering

## Goal

Build the Thread rendering section of `docs/agents/todo.md`. Today the thread shows every block as
plain text: an agent message is its raw Markdown, a thought is a grey paragraph, and a tool call is
its tool call title alone, with no way to see what the tool produced. When this is done:

- An agent message renders as Markdown with `react-markdown` and `remark-gfm`, with a copy button
  under it that copies its Markdown source.
- A thought is a collapsed "Thinking" row. Clicking it shows the thought's text; clicking again
  hides it.
- A tool call of tool kind `execute` is a Run Command block: a filled block with the label "Run
  Command" and the tool call title. Any other tool call is a row with an icon for its tool kind and
  its tool call title. Clicking either shows the tool call's content beneath it: its text content,
  and its diffs as a path and new text, the same way a pending permission request shows content
  today. Clicking again hides it.
- A rejected prompt's turn error entry renders as a bordered box with an icon directly under the
  user message, as in the thread wireframe.

The daemon, the wire protocol, and the core do not change.

## Related code

- `app/src/transcript/blocks.ts` — `Block`. The `tool_call` block gains the tool call's content.
- `app/src/transcript/reduce.ts` — the transcript reducer. `tool_call` carries `content`, and
  `tool_call_update` carries `content` as a replacement of the whole list, or `null` or absent to
  leave it unchanged, per `ToolCallUpdate` in
  `app/node_modules/@agentclientprotocol/sdk/dist/schema/types.gen.d.ts`.
- `app/src/transcript/reduce.test.ts` — `tool_call_updates_merge_by_id`, the table-driven test the
  content cases join.
- `app/src/components/Thread.tsx` — `ItemView`, which renders each block as a `div` today.
- `app/src/components/Permission.tsx` — `ContentView`, which renders `ToolCallContent` items: text
  content as preformatted text, a diff as its path and new text, and nothing for terminal or
  non-text content. Tool calls need the same rendering, so it moves to a shared component.
- `app/src/App.tsx` — `Session` memoizes the items, so the thread's scroll-to-bottom effect runs
  only when the items change. Expanding a row does not change the items.
- `app/src/styles.css` — the `.block` rules. `.block` sets `white-space: pre-wrap`, which rendered
  Markdown must override.
- `docs/agents/wireframes/thread.png` and `main-window.png` — the rendering to match.
- `~/projects/ox/src/acp/convert.rs` — for reference only: Ox sends a tool call's outcome, for shell
  commands and edits alike, as one text content item, so expanding a Run Command block shows the
  command's output and expanding an edit shows its outcome text. ur relies on the ACP fields, not on
  this.

## Decisions

### Tool call content lives on the block

The `tool_call` block gains `content: ToolCallContent[]`, the ACP content list unchanged. A
`tool_call` update sets it, `[]` when absent. A `tool_call_update` with a content array replaces it,
and one with `null` or no `content` leaves it alone, which is what ACP means by "replace the content
collection". A `tool_call_update` for an unknown ID starts its block with the update's content or
`[]`.

The block keeps the ACP `ToolCallContent` items rather than text derived from them: `Permission`
already renders that type from the request's `toolCall.content`, and one shared component renders
both. The transcript reducer stays the only code that reads `SessionUpdate` shapes.

### One component for tool call content

`ContentView` moves from `Permission.tsx` to `app/src/components/ToolCallContent.tsx` as
`ToolCallContentView({ content })`, taking the whole list. It renders each text content item as
preformatted text, each diff as its path and new text, and skips terminal and non-text content, as
today. `Permission` and the tool call rows use it.

### Rows keep their expanded state in component state

`Thought` and `ToolCall` each hold `expanded` in a `useState`, collapsed at first. `Thread` keys
items by index, and blocks only append or change in place, so a row keeps its state while its block
updates. A tool call block that becomes a permission item while its request is pending, and a tool
call block again once it is resolved, comes back collapsed; that is fine.

A row is always clickable. A tool call with no content yet, such as one still `in_progress`, expands
to nothing.

### Rendering

- `Thought`, in `Thread.tsx`: a row with a glyph and the label "Thinking". Expanded, the thought's
  text follows in the dim colour with a left border, as in the wireframe, as plain text, not
  Markdown.
- `ToolCall`, in `app/src/components/ToolCall.tsx`: for tool kind `execute`, a Run Command block: a
  filled block with the small dim label "Run Command" and the tool call title in the buffer font,
  with the content beneath the title inside the same block when expanded. For any other tool kind,
  or none, a row with the glyph for the kind and the tool call title, with the content beneath when
  expanded. `toolIcon(kind)` in the same file is a fixed table over ACP's ten tool kinds, since the
  set is closed:

  | Tool kind     | Glyph |
  | ------------- | ----- |
  | `read`        | ⌕     |
  | `search`      | ⌕     |
  | `edit`        | ✎     |
  | `delete`      | ✕     |
  | `move`        | ⇢     |
  | `fetch`       | ⇣     |
  | `think`       | ✧     |
  | `switch_mode` | ⇄     |
  | `other`       | •     |

  `execute` has no glyph. The Thinking row uses the `think` glyph. Glyphs are Unicode text, no icon
  library; adjust a glyph during the work if it renders as an emoji.

- `AgentMessage`, in `app/src/components/AgentMessage.tsx`: `<Markdown remarkPlugins={[remarkGfm]}>`
  over the block's text, then a right-aligned copy button with the ⧉ glyph that calls
  `navigator.clipboard.writeText(text)`. The component is wrapped in `memo`: the reducer keeps the
  identity of blocks it does not touch, so earlier messages are not re-parsed on every chunk of the
  current one.

  The copy button relies on the webview allowing the clipboard API. Check it in the running app
  during the work. If the webview refuses it, add `tauri-plugin-clipboard-manager` and call its
  `writeText` instead; nothing else changes.

- The `error` block renders as a bordered box with a ⓘ glyph before the message. The existing
  `.block.user + .block.error` rule keeps a rejected prompt's error directly under its user message;
  a failed turn's error keeps the ordinary spacing.

- `styles.css`: `.block.agent` gets `white-space: normal` and rules for the Markdown elements:
  paragraph and list margins, `code` and `pre` in the buffer font on the filled background, tables
  from GFM, headings, and blockquotes. The Run Command block, the tool call rows, the expanded
  content, and the error box get rules matching the wireframe.

### Dependencies

`react-markdown` 10.1.0 and `remark-gfm` 4.0.1 join `app/package.json`'s dependencies.

## Naming

- Agent message, thought, "Thinking" row, tool call, tool call title, tool kind, tool call status,
  Run Command block, transcript reducer, rejected prompt, turn error entry, user message, thread —
  as defined in `docs/agents/glossary.md`.
- Tool call content — the ACP `ToolCallContent` items of a tool call: text content, diffs, and
  terminal content. The block's `content` field and `ToolCallContentView` use this name.
- Expanded — a Thinking row or tool call showing its content. `expanded` in code.
- Copy button — the button under an agent message that copies its Markdown source.
- `Thought`, `ToolCall`, `AgentMessage`, `ToolCallContentView` — the components.
- `toolIcon()` — the glyph table by tool kind.

## Test plan

Unit tests under vitest, in `app/src/transcript/reduce.test.ts`:

- `tool_call_updates_merge_by_id` gains cases: "content replaced", where a `tool_call` with one text
  item followed by a `tool_call_update` with two items holds the two; "a null content left
  unchanged", where an update with `content: null` keeps the original item; and the existing cases
  gain `content: []` in their expected blocks. The "an unknown ID appending a block" case shows the
  new block with `content: []`.

Components stay untested under vitest, as today; there is no DOM in the unit tests. The end-to-end
suite tests them.

The fake server gains a `render` script: a thought, a completed `execute` tool call titled
`ls *.tally` with output `a.tally` and `b.tally`, a completed `read` tool call titled `read a.tally`
with content `one tally`, and the Markdown message `**Two** tallies:` followed by a list of
`a.tally` and `b.tally` in inline code. End-to-end tests in `app/e2e/thread.test.ts` prompt
`render`:

- `agent messages render as Markdown`: the bold text and the list's inline code.
- `the copy button copies the message's Markdown source`: `Gui.clickCopy()` records what the button
  writes to the clipboard, since the system clipboard is the user's.
- `a Thinking row shows its thought when clicked and hides it when clicked again`.
- `a Run Command block shows its output when clicked`.
- `a tool call row shows its content when clicked`.

The daemon restart test compares the agent message's paragraphs, since the agent block now holds the
copy button too.

Run the end-to-end suite at the end, since this finishes a section of `docs/agents/todo.md`. Run the
check in `docs/agents/todo.md` with Ox by hand: in a session whose turn thinks, edits a file, and
runs a shell command, confirm the Thinking row, the edit row with its pencil glyph, and the Run
Command block, that each expands and collapses on click, that the edit's outcome text and the
command's output appear expanded, that the agent message renders Markdown and its copy button puts
the Markdown source on the clipboard, and, after a prompt the server rejects, that the error box
sits directly under the user message.

## Implementation plan

1. `pnpm -C app add react-markdown remark-gfm`.
2. `app/src/transcript/blocks.ts` and `reduce.ts`: `content` on the `tool_call` block, set by
   `tool_call` and replaced by `tool_call_update`, with the tests in `reduce.test.ts`.
3. `app/src/components/ToolCallContent.tsx`: `ToolCallContentView`, moved from `Permission.tsx`,
   which now imports it.
4. `app/src/components/ToolCall.tsx`: `ToolCall` and `toolIcon()`.
5. `app/src/components/AgentMessage.tsx`: `AgentMessage` with Markdown and the copy button.
6. `app/src/components/Thread.tsx`: `Thought`; `ItemView` renders `AgentMessage`, `Thought`,
   `ToolCall`, and the error box.
7. `app/src/styles.css`: the Markdown, Thinking row, tool call row, Run Command block, expanded
   content, copy button, and error box rules.
8. `crates/ur-fake-server/src/lib.rs`: the `render` script. `app/e2e/harness.ts`: `Gui.clickCopy()`.
   `app/e2e/thread.test.ts`: the tests in the Test plan. `session.test.ts`: the daemon restart test
   compares paragraphs.
9. Run the end-to-end suite and the check in `docs/agents/todo.md` with Ox, including the copy
   button.

## Documentation updates

- `AGENTS.md`: map `app/src/components/AgentMessage.tsx`, `ToolCall.tsx`, and `ToolCallContent.tsx`;
  say `Thread.tsx` holds the Thinking row; say `Permission.tsx` renders its content through
  `ToolCallContentView`; say `blocks.ts`'s tool call block carries the tool call content.
- `docs/agents/architecture.md`: under GUI architecture, say the transcript reducer keeps each tool
  call's content, replaced whole by `tool_call_update`, and that expanded rows are component state.
- `docs/agents/glossary.md`: under Transcripts, add Tool call content and Expanded as defined above;
  under Thought, say clicking the "Thinking" row shows the text.
- `docs/agents/todo.md`: check off Thread rendering.
- `docs/agents/testing.md`: the thread rendering guarantees.
- `AGENTS.md`: map `app/e2e/thread.test.ts`.
