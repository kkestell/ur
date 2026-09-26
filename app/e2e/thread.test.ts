import assert from "node:assert/strict";
import { setTimeout as sleep } from "node:timers/promises";
import { type Gui, type TestEnvironment, e2eTest } from "./harness.ts";

/** Opens the GUI on a new session and runs the fake server's `render` script. */
async function render(environment: TestEnvironment): Promise<Gui> {
  await environment.newSession();
  const gui = await environment.openGui();
  await gui.click(".sidebar .row", "New session");
  await gui.sendPrompt("render");
  await gui.waitForText(".block.agent", "tallies");
  return gui;
}

e2eTest("agent messages render as Markdown", async (environment) => {
  const gui = await render(environment);
  assert.deepEqual(await gui.texts(".block.agent strong"), ["Two"]);
  assert.deepEqual(await gui.texts(".block.agent li code"), ["a.tally", "b.tally"]);
});

e2eTest("the copy button copies the message's Markdown source", async (environment) => {
  const gui = await render(environment);
  // The system clipboard is the user's, so the test records what is written.
  const copied = await gui.clickCopy(".block.agent .copy");
  assert.equal(copied, "**Two** tallies:\n\n- `a.tally`\n- `b.tally`\n");
});

e2eTest("a Thinking row shows its thought when clicked and hides it when clicked again", async (environment) => {
  const gui = await render(environment);
  await gui.waitForNone(".thought-text");
  await gui.click(".thought-row", "Thinking");
  await gui.waitForText(".thought-text", "weighing the tallies");
  await gui.click(".thought-row", "Thinking");
  await gui.waitForNone(".thought-text");
});

e2eTest("a Run Command block shows its output when clicked", async (environment) => {
  const gui = await render(environment);
  await gui.waitForText(".run-command-header", "ls *.tally");
  await gui.waitForNone(".run-command .tool-call-content");
  await gui.click(".run-command-header", "ls *.tally");
  await gui.waitForText(".run-command .tool-call-content", "a.tally\nb.tally");
});

e2eTest("a tool call row shows its content when clicked", async (environment) => {
  const gui = await render(environment);
  await gui.waitForNone(".tool-call .tool-call-content");
  await gui.click(".tool-call-row", "read a.tally");
  await gui.waitForText(".tool-call .tool-call-content", "one tally");
});

e2eTest("clicking a link in an agent message leaves the app in place", async (environment) => {
  await environment.newSession();
  const gui = await environment.openGui();
  await gui.click(".sidebar .row", "New session");
  // The fake server repeats the prompt, link and all. A relative link opens
  // no browser.
  await gui.sendPrompt("see [main](src/main.rs)");
  const app = await gui.url();
  await gui.click(".block.agent a", "main");
  // A navigation would replace the app within this time.
  await sleep(1000);
  assert.equal(await gui.url(), app);
  assert.ok(await gui.hasElement(".sidebar"), "the app is gone");
});
