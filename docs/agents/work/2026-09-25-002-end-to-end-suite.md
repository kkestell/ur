# End-to-end suite

## Plan

`docs/agents/plans/2026-09-25-002-end-to-end-suite.md`

## Summary

`pnpm -C app e2e` builds the daemon and the app into `target/e2e`, runs the two terminal guarantees
against a fresh test environment per test, and saves a screenshot for each failure. The plan's goal
is met. Both guarantees gained an owning test in `app/e2e/terminal.test.ts`; no Rust test gained,
lost, or moved a guarantee.

## Departures from the plan

- `Gui.waitForLine` also takes a pattern, matched anywhere in a row, so the `top` test can find its
  `Processes:` header. A string must equal the row without its surrounding spaces, which
  `:set lines?` needs: vim indents its answer.
- `app/pnpm-workspace.yaml` declines the build scripts of `edgedriver` and `geckodriver`, which
  `webdriverio` depends on. Without an answer for them, `pnpm install` exits with an error. The
  tests never use those drivers.
- `app/e2e/tsconfig.json` also sets `target` and `lib` to ES2023, for `findLast` and `replaceAll`,
  and `erasableSyntaxOnly`, so `tsc` rejects syntax Node's type stripping cannot run.
- `AGENTS.md` and `docs/agents/testing.md` say "when finishing each section of
  `docs/agents/todo.md`" instead of "at the end of each milestone", following the `AGENTS.md` rule
  against naming milestones outside `todo.md`.

## Decisions

- `Gui.connect` waits for the webview to leave `about:blank` before its first script. The plugin's
  `/status` is ready once the window exists, and its synchronous `execute` stores the result in a
  page global that it polls. A script run before the app's page loads loses that global and waits
  out the 30-second script timeout.
- `TestEnvironment.stop` waits for the daemon to exit and retries the whole directory removal. After
  SIGHUP, bash and vim still write `.bash_history` and `.viminfo`; `rmSync`'s own `maxRetries`
  retries only the final `rmdir`, so it failed with `ENOTEMPTY` in about one run in eight.
- `openGui` waits for the daemon socket to accept a connection before starting the app, because the
  core connects once and does not retry yet.
- Closing and stopping use SIGKILL, so a GUI never runs shutdown code the daemon would not see from
  a lost window.

## Automated checks

- `cargo fmt --all -- --check` — passed.
- `cargo test --workspace --all-targets --all-features` — passed.
- `cargo build --workspace --all-features` — passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — passed.
- `pnpm -C app build` — passed.
- `pnpm -C app e2e` — passed 4 times in a row. The test step alone
  (`node --test --test-concurrency=1 "e2e/*.test.ts"`) passed 12 more times in a row after the
  removal fix.

## Manual verification

1. The `top` test catches a restored application that leaves its last screen behind.

   With `ENTER_ALTERNATE_SCREEN` in `crates/ur/src/daemon/terminal.rs` set to `b""`:

   ```sh
   pnpm -C app e2e
   ```

   `quitting a restored full-screen application returns to the shell` failed with `top`'s header and
   process list mixed into the shell's rows, and saved its screenshot. The vi test passed. The
   constant was restored and the suite passed again.

## Follow-up work

- One run failed with "timed out waiting for the terminal to render" when the GUI reopened after
  `top`, with a blank window in the screenshot. It did not recur in about 30 later suite runs or 40
  ad-hoc repetitions of the same steps. If it returns, record `document.visibilityState` and whether
  the app shows its error text when the wait times out.
