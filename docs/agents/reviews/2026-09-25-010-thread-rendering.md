# Thread rendering

## Scope and coverage

Reviewed commit `16f7f46`, its transcript and permission paths, and the Thread rendering plan and
work log. Used correctness, API design, testing, readability, security, and simplicity lenses. The
work log's WebDriver check confirms the link behavior; I did not rerun an interactive GUI check.

## Fixed

- **A permission request hides its tool call's content** (`app/src/transcript/permissions.ts:27`):
  When a request omitted content, it replaced a matching tool call row and hid content already in
  that row. The permission item now uses the request's content when supplied and otherwise keeps the
  tool call block's content. A focused test covers both cases, and the end-to-end test
  `a permission request shows its tool call's content` covers the GUI, with the fake server's tool
  calls now carrying content their permission requests omit.

## Findings

### High

#### Correctness

- **OX-0007 Agent message links replace the app with a web page**
  (`app/src/components/AgentMessage.tsx:17`): A normal click on a link in an agent reply navigates
  the Tauri webview away from ur, so the app disappears until it restarts. The work log reproduced
  this with an `https://example.com/` link, and `AgentMessage` leaves Markdown links with their
  default navigation behavior. Decide whether links should open in the default browser or be inert,
  then handle clicks without navigating the app's webview.

## Checks run

- `make check` passed outside the sandbox: Rust formatting, tests, build, and Clippy; app build and
  25 unit tests; and `dprint check`. The first sandboxed run stopped at the Rust daemon tests
  because socket operations returned `Operation not permitted`.
- `make format-docs`, `make check-docs`, and `git diff --check` passed. Dprint reported that it
  could not write its incremental cache in the sandbox.
- `make e2e` passed, 24 tests.
- Inspected the complete test diff and the work log's interactive check. The new permission-content
  test owns the fallback and replacement behavior; no other test guarantee changed in this review.

## Verdict

One content bug is fixed. Agent message links still need a navigation decision and fix before the
Thread rendering task is fully reliable.
