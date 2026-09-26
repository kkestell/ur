# Ordered image attachments

## Goal

Fix OX-0010 in `docs/agents/issues.csv`. A prompt must include every image attachment still shown in
the editor when the user sends it, in drop order. The editor must not send while an image is still
being read. Removing an image attachment during its read must leave it out of the prompt even if the
read finishes later.

## Related code

- `app/src/components/Editor.tsx` — `onDrop` starts independent `FileReader` operations and appends
  images when they finish; `send` and Enter use only completed attachments. The chip's Remove button
  currently identifies an image by array position.
- `app/e2e/harness.ts` — `Gui.dropFile()`, `sendPrompt()`, and `pressKey()` drive the editor.
- `app/e2e/editor.test.ts` — the current image test waits for a chip before sending, so it does not
  cover a pending read or reversed completion order.

## Decisions

Reserve an image attachment's position as soon as its file is dropped, including across separate
drops. Give each attachment an identity so a read completion updates only its own position. Show a
chip while it loads and let Remove discard it. Ignore a result for an attachment that has been
removed. On read failure, remove that attachment and show the existing file error message.

Disable Send while any remaining image attachment is loading, and make Enter follow the same rule.
Keep the text and image attachments in the editor until a complete prompt can be sent. After all
reads finish, build the prompt from the ready attachments in their reserved order. The existing
prompt error and busy response restoration should still restore the sent attachments.

## Naming

- **Image attachment** — an image file dropped on the editor, read by the webview and sent as image
  content in the next prompt, as defined in `docs/agents/glossary.md`. It has a loading state until
  its data is ready.
- **Prompt** — the ACP `session/prompt` content built by the editor. Use the existing name in code.

## Test plan

- Extend `Gui` in `app/e2e/harness.ts` to drop several files in one event and to control when the
  browser's `FileReader` starts each queued read. This makes the races deterministic without
  depending on image size or timing.
- In `app/e2e/editor.test.ts`, hold a dropped image's read, type text, and try Enter. Assert the
  editor keeps the draft and no user message appears. Release the read, send, and assert the prompt
  has the text and image. Check that Send is disabled while the read is pending.
- In `app/e2e/editor.test.ts`, drop two distinct images together, release the second read before the
  first, and assert the chips and the sent user message's thumbnails keep the drop order. Also
  remove a pending attachment, release its read, and confirm it does not join the next prompt.
- Keep the existing test for a non-image drop and the current image prompt test. Add the two new
  observable guarantees to the end-to-end guarantee list in `docs/agents/testing.md`.

## Implementation plan

1. In `app/src/components/Editor.tsx`, keep image attachments in drop order with a stable identity
   and loading or ready state. Reserve positions before starting reads, update each position on
   success, and remove that position on read failure. Render and remove loading attachments like
   ready ones.
2. In `app/src/components/Editor.tsx`, guard both Send and Enter while any remaining attachment is
   loading. When all are ready, build the prompt in attachment order and preserve the existing
   response restoration behavior.
3. Add the browser read controls and multi-file drop helper in `app/e2e/harness.ts`, then add the
   regression tests in `app/e2e/editor.test.ts`.
4. Update `docs/agents/testing.md` with the new guarantees, check off OX-0010 in
   `docs/agents/todo.md`, and set its status to `fixed` in `docs/agents/issues.csv` when the fix is
   implemented.
