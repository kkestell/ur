# Thread rendering

## Plan

`docs/agents/plans/2026-09-25-009-thread-rendering.md`

## Summary

Agent messages render as Markdown through `react-markdown` and `remark-gfm`, each with a copy button
that copies its Markdown source. Thoughts are collapsed "Thinking" rows, tool calls of kind
`execute` are Run Command blocks, and other tool calls are rows with the glyph for their tool kind;
clicking one shows its content and clicking again hides it. The transcript reducer keeps each tool
call's content, and `ToolCallContentView` renders it for tool calls and pending permission requests
alike. A turn error is a bordered box with ⓘ, and a rejected prompt's sits 6px under its user
message. The daemon, the wire protocol, and the core are unchanged. The plan's goal is met: the
check in `docs/agents/todo.md` passes with Ox.

Test guarantees: `tool_call_updates_merge_by_id` gains "content replaced" and "a null content left
unchanged", and its other cases expect `content: []`. The tool call block in `permissions.test.ts`
gains `content: []`, since `Block` now requires it and `tsc` checks the tests; its guarantees are
unchanged. The five end-to-end tests in `thread.test.ts` own the rendering guarantees, against the
fake server's new `render` script. The daemon restart test compares the agent message's paragraphs,
since the agent block now holds the copy button, and keeps its guarantee.

## Decisions

- Only the row, or the Run Command block's label and title, toggles a tool call or thought. The
  expanded content does not, so selecting text in it does not collapse it.
- A tool call without a tool kind gets the `other` glyph: `other` is ACP's default tool kind.
- Each tool call content item is its own box: bordered under a tool call row, ruled off inside the
  Run Command block, and the filled box permission requests already used. A tool call whose content
  has nothing to render expands to nothing.
- The error box keeps the red text the error already had; the wireframe draws it in the default text
  color.
- `.block.user + .block.error` leaves a 6px gap and no longer overrides the padding: two bordered
  boxes pulled flush would read as one double border, and the wireframe also leaves a gap smaller
  than the ordinary spacing.
- `navigator.clipboard.writeText` works in the webview, so `tauri-plugin-clipboard-manager` was not
  added.

## Automated checks

- `cargo fmt --all -- --check` — passed.
- `cargo test --workspace --all-targets --all-features` — passed; no bindings changed.
- `cargo build --workspace --all-features` — passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — passed.
- `pnpm -C app build` — passed. Vite's warning about chunks over 500 kB was already there at HEAD;
  the bundle grew from 567 kB to 723 kB with `react-markdown` and `remark-gfm`.
- `pnpm -C app test` — passed, 24 cases.
- `dprint check` — passed.
- `pnpm -C app e2e` — passed, 23 tests.

## Manual verification

1. The check in `docs/agents/todo.md` with Ox, driven through the WebDriver build in `target/e2e` by
   an ad-hoc script. It starts a daemon in a temporary directory with an Ox config file, adds a
   workspace holding `notes.txt`, creates a session with `ur new`, opens `ur-app`, and drives it
   through WebDriver.

   ```sh
   make e2e
   node --experimental-strip-types /tmp/ur-adhoc/thread.ts
   ```

   A prompt asking Ox to think, edit `notes.txt`, run `cat notes.txt && ls -la`, and reply in
   Markdown showed ✧ Thinking rows, ⌕ rows for "Read notes.txt" and "Find files matching *", the ✎
   row "Apply patch to notes.txt", and the Run Command block for `cat notes.txt && ls -la`, all
   collapsed. Clicking each expanded it: the thought's text behind a left border, "Applied patch.
   Modified notes.txt" under the edit, and the exit code and output under a rule inside the Run
   Command block. Clicking again collapsed every one. The reply rendered as a heading, a list,
   inline code, a code block, a table, and a blockquote. Its copy button's `writeText` resolved, and
   `pbpaste` returned the message's Markdown source exactly, as rebuilt from `ur read`. WebDriver
   clicks are script `click()` calls, so the clipboard API needs no user gesture here. The pending
   permission request for the command showed its "Working directory" content in the same filled box
   as before. A 7 MB prompt showed "Invalid params: prompt exceeds the model context limit" in the
   bordered box with ⓘ, 6px under the user message.

2. Links and glyphs, against the fake server, whose reply echoes the prompt.

   ```sh
   node --experimental-strip-types /tmp/ur-adhoc/link.ts
   ```

   All ten glyphs, ⌕ ✎ ✕ ⇢ ⇣ ✧ ⇄ • ⧉ ⓘ, render as text, not emoji. Clicking the link in the reply
   "you said: see [the example](https://example.com/) page" navigated the webview from
   `tauri://localhost` to `https://example.com/`, and the app was gone until it restarted.

## Follow-up work

- Clicking a link in an agent message navigates the whole webview to the linked page, replacing the
  app until it restarts. Links also render in the default link blue. They need to open in the
  default browser or do nothing.
- The wireframes show the copy button only under a turn's last agent message. As the plan says,
  every agent message has one, including the short messages Ox sends between tool calls.
