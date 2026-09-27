# Full codebase review

## Scope and coverage

Reviewed the Rust daemon, CLI, socket client, fake server integration, Tauri core, webview, and
their contracts and relevant tests. Applied correctness, error handling, concurrency, resources, API
design, testing, architecture, simplicity, documentation, security, Rust ownership, Rust idioms, and
Cargo lenses. The GUI was not exercised interactively; findings were confirmed by tracing the
reachable paths and existing tests.

## Findings

### High

#### Correctness

- **OX-0010 Dropped images can be omitted or reordered in a prompt**
  (`app/src/components/Editor.tsx:159`): `onDrop` starts one asynchronous `FileReader` for each
  image and appends each result when it finishes. `send` at line 66 immediately builds the prompt
  from attachments already loaded. Sending text while a large image is still loading sends the text
  without that image; its eventual result becomes an attachment to the next prompt. Several dropped
  images can also arrive in a different order from the drop. The existing end-to-end test waits for
  the chip before sending, so it does not cover this path. Track pending reads, preserve file order,
  and keep Send from using an incomplete set of attachments. Add a GUI test that sends during a
  pending read and one that drops differently sized images together.

### Medium

#### Correctness

- **OX-0011 Followed CLI transcript misses replayed entries after a server restart**
  (`crates/ur/src/cli/read.rs:40`): The daemon sends a replacement `SessionSnapshot` when it reloads
  a subscribed session after the server restarts, as
  `subscribed_sessions_reload_after_the_server_restarts` confirms. `ur read --follow` prints the
  initial snapshot, then only `Entry` events. It discards every later snapshot, so replayed entries
  that were not in the first transcript never appear in the followed output. Define how a
  line-oriented follower represents transcript replacement, then handle later snapshots and test the
  restart path.

### Low

#### Architecture

- **OX-0012 The model picker interprets an Ox description as structured metadata**
  (`app/src/components/ConfigPicker.tsx:113`): The picker hides any value description exactly equal
  to `Accepts images` and draws an image glyph. This depends on Ox wording even though the ACP
  boundary says server-provided labels and descriptions have no special meaning to ur. A different
  server can describe the same capability differently and show no glyph; an unrelated value with
  that description gets one. Keep descriptions as text until there is structured capability data or
  make the Ox-specific display rule an explicit product decision.

## Checks run

- Traced each finding through its caller, event path, and relevant existing test.
- `make check-docs` — passed.
- Focused `git diff` and issue-link inspection — passed.

## Verdict

No straightforward fix was available without choosing new user-visible behavior. Plan the image
attachment behavior first, then the CLI snapshot format. The model description rule is a smaller
compatibility issue.
