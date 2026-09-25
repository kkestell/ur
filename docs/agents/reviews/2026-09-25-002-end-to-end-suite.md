# End-to-end suite review

## Scope and coverage

Reviewed commit `46a6434` against `docs/agents/plans/2026-09-25-002-end-to-end-suite.md` and its
work log: `app/e2e/harness.ts`, `app/e2e/terminal.test.ts`, `app/e2e/tsconfig.json`, the `e2e`
script and dependencies in `app/package.json`, `app/pnpm-workspace.yaml`, `.gitignore`, and the
updates to `AGENTS.md`, `docs/agents/testing.md`, `docs/agents/glossary.md`, and
`docs/agents/todo.md`. Also read the code the suite depends on: the `webdriver` feature in
`app/src-tauri/`, `TerminalPane.tsx`, `gui_state.rs`, and how the daemon starts shells in
`terminal.rs`.

Lenses: correctness, error-handling, testing, documentation.

The end-to-end suite itself was not run for this review; `pnpm-lock.yaml` was not reviewed.

## Findings

### Medium

#### Error-handling

- **Stopping a test environment after the daemon has exited never finishes**
  (`app/e2e/harness.ts:89`): `stop()` calls `once(this.#daemon, "exit")` only when it stops the
  environment. If the daemon has already exited, for example because it crashed during the test,
  which is a failure the suite exists to catch, the `exit` event has already fired and the promise
  never resolves. The test's own error is lost, and the temporary directory is never removed. A
  minimal `node:test` reproduction (spawn `true`, throw, then `await once(child, "exit")` in
  `finally`) reports the test as cancelled with "Promise resolution is still pending but the event
  loop has already resolved" instead of the thrown error. `Gui` avoids this by creating its `exited`
  promise when it spawns the process. Do the same for the daemon: create the exit promise in the
  constructor and await it in `stop()`.

#### Testing

- **Nothing checks that the GUI reopens at a different size** (`app/e2e/terminal.test.ts:10`): the
  guarantee is that the terminal survives reopening at a different window size and that the editor
  sees the new size. The test sets 700×450, then reopens at the default size and checks that vim's
  `lines` equals the new row count. But it never checks that the first window actually had fewer
  rows. If `setWindowSize` stops resizing the window (a plugin change, or a window size limit), both
  windows have the same size. The daemon then never has to resize on reattach, and the test still
  passes. After `setWindowSize`, wait for the row count to change, record it, and assert that the
  reopened view's row count differs.

### Low

#### Error-handling

- **A failed screenshot replaces the test's error** (`app/e2e/harness.ts:207`): `openGui` adds the
  `Gui` to the environment's list before it connects. If the WebDriver server never answers or
  `remote()` fails, the process is still running, so `screenshot()` picks that GUI. Then
  `#session()` throws "the GUI has no WebDriver session", and that error replaces the original
  timeout or connection error. Catch errors from `environment.screenshot(name)` in `e2eTest`, so the
  test's own error is the one thrown.
- **`waitForLine` hides WebDriver errors** (`app/e2e/harness.ts:168`): the `catch` turns every error
  into "no line … on screen", including errors thrown by `lines()` itself, such as a script timeout
  or a lost session. The rows shown are then from an earlier poll, or empty. Pass the caught error
  as `cause`, or only rewrite the timeout from `waitFor`.

## Unresolved questions

- The plan says Node 22 strips TypeScript types by default. That may be true only for Node 22.18 and
  later, and `app/package.json` declares no `engines` version. It was checked only on Node 22.22.1.
  Running `pnpm -C app e2e` on an earlier Node 22 release would show whether a minimum version needs
  to be declared.

## Checks run

- `pnpm -C app exec tsc -p e2e` — passed.
- A `node:test` reproduction of awaiting `once(child, "exit")` after the child had exited — the test
  was reported as cancelled, and its thrown error was lost.
- Traced `openGui`, `e2eTest`, and `stop` for the failure paths above. Also confirmed that the shell
  starts in the test's `HOME`, so the `notes.txt` check looks in the right directory, and that the
  GUI state file does not store the window size, so reopening uses the default size.

## Verdict

The implementation matches the plan, and the documentation updates are complete and consistent. Two
changes are needed before the suite can be relied on to report failures: stopping must finish when
the daemon has already exited, and the reopen test must assert that the window size actually
changed. The two low-severity error-handling fixes are small and would make failures easier to
diagnose.
