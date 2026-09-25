# Attention in the GUI

## Scope and coverage

Commit `c9ebfc4`, which shows attention in the GUI: the attention ordering, counts, and status marks
in the watch store and sidebar, the core's focus path in `app/src-tauri/src/link.rs`, `main.rs`, and
`commands.rs`, the pending permission requests placed among the blocks by
`app/src/transcript/permissions.ts` and rendered by `Permission.tsx` and `Thread.tsx`, the shortcuts
in `app/src/keys.ts`, the `Session` component in `App.tsx`, the styles, and the documentation
updates. Reviewed against the plan in `docs/agents/plans/2026-09-25-008-attention-in-the-gui.md` and
its work log. Read the watch and session stores, the daemon's startup order in `daemon/mod.rs` and
`daemon/acp.rs`, and the earlier review of the agent GUI for the code the change depends on.

Lenses: correctness, comments, simplicity, testing, and documentation.

Not covered: the Ox check in `docs/agents/todo.md`, which the work log reports as passed by hand.
The end-to-end suite was not run again after the fixes; they do not touch the terminal path or the
harness.

## Fixed

- **Any watch event scrolls the selected thread to the bottom** (`app/src/App.tsx:134`): `Session`
  renders on every watch event, since it reads `useWatch()`, and built a fresh items array each
  render, so `Thread`'s scroll effect, keyed on the items, fired whenever any session in any
  workspace changed status. Scrolling up to read an earlier reply while another session's turn ended
  jumped the view back to the bottom. The same fresh `[]` for the requests re-installed the
  `keydown` listener each render. The items are now memoized on the thread and the requests, and the
  no-requests case shares one array. The end-to-end test
  `another session's activity leaves the thread's scroll position alone` covers it.
- **The focus replay comment gives the wrong reason** (`app/src-tauri/src/link.rs:108`): it said
  focus is sent after the subscriptions so the daemon has listed every saved session first. The
  subscriptions and the focus are all spawned tasks with no ordering between them, and the daemon
  lists every workspace's sessions before it accepts a socket connection at all. The comment now
  says that.

## Findings

### Low

#### Correctness

- **OX-0006 An interrupted turn reloaded from the server reads as a rejected prompt**
  (`app/src/styles.css:219`): the adjacency rule pulls any error block directly after a user block
  up under it. When the server exits during a turn and the session is reloaded, a replay that omits
  the uncommitted agent output leaves the interruption error right after the user message, so it
  looks like the prompt was rejected. The work log notes this. Suggested fix: decide in the daemon
  or the reducer whether an error is a rejection, and style only that case.

## Checks run

- `make check` — passed: `cargo fmt`, `cargo test`, `cargo build`, `cargo clippy`,
  `pnpm -C app build`, `pnpm -C app test` (22 cases), and `dprint check`.
- `make e2e` — passed, 18 tests.

## Verdict

The change does what the plan says, and the focus path and the permission rendering are sound. Two
findings were fixed in place; one low severity finding is open.
