# Multiple ACP servers

## Plan

`agents/plans/2026-09-26-004-multiple-acp-servers.md`

## Summary

The daemon now runs named ACP servers independently and keeps their sessions distinct even when
their ACP session IDs match. The GUI provides per-server settings, creation choices, labels, and
capability-dependent controls. The one-shot client selects a configured server by name. The plan's
goal is met.

## Departures from the plan

- The original menu test method tried to intercept Tauri's frozen IPC function. The plan was
  corrected to wrap `Menu.new` only in the end-to-end build, retain the real menu construction, and
  invoke the registered handlers in place of the native popup. Confirmation dialogs use a similar
  end-to-end wrapper.

## Decisions

- Server removal and connection exit settle pending load and delete operations before discarding
  sessions. A settings form refreshes when a newly added server first appears in the watch state, so
  a failed launch leaves its configuration available to correct.

## Automated checks

- `make check` — passed after correcting three Clippy style findings.
- `make e2e` — passed, 85 tests.
- `make check-docs` — passed.
