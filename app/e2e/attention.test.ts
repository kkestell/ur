import assert from "node:assert/strict";
import { e2eTest } from "./harness.ts";

e2eTest("a working session shows the spinner", async (environment) => {
  const session = await environment.newSession();
  const gui = await environment.openGui();
  await environment.ur("prompt", session, "hold");
  await gui.waitForText(".sidebar .row:has(.status.spinner)", "New session");
});

e2eTest("a session waiting for permission shows its mark and its workspace's count", async (environment) => {
  const session = await environment.newSession();
  const gui = await environment.openGui();
  await environment.ur("prompt", session, "tool");
  await gui.waitForText(".sidebar .row:has(.status.dot)", "New session");
  await gui.waitForText(".workspace-name .count", "1");
});

e2eTest("clicking a permission option answers the request", async (environment) => {
  await environment.newSession();
  const gui = await environment.openGui();
  await gui.click(".sidebar .row", "New session");
  await gui.sendPrompt("tool");
  await gui.waitForText(".block.permission", "count the tallies");
  await gui.waitForText(".block.awaiting", "Awaiting Confirmation.");
  await gui.click(".block.permission .option", "Go ahead");
  await gui.waitForText(".block.agent", "selected go");
  await gui.waitForNone(".block.awaiting");
  await gui.waitForNone(".sidebar .status");
});

e2eTest("a permission request shows its tool call's content", async (environment) => {
  await environment.newSession();
  const gui = await environment.openGui();
  await gui.click(".sidebar .row", "New session");
  // The request carries no content; its tool call does.
  await gui.sendPrompt("tool");
  await gui.waitForText(".block.permission .tool-call-content", "every *.tally file");
});

e2eTest("the permission shortcuts answer the oldest request", async (environment) => {
  await environment.newSession();
  const gui = await environment.openGui();
  await gui.click(".sidebar .row", "New session");
  // `tools` asks for tally-1 and tally-2 at once.
  await gui.sendPrompt("tools");
  await gui.waitForText(".block.permission", "count the tallies");
  await gui.pressShortcut("KeyY");
  await gui.pressShortcut("KeyZ", { alt: true });
  await gui.waitForText(".block.agent", "tally-1: go, tally-2: stop");
});

e2eTest("permission labels and shortcuts use Ctrl on other platforms", async (environment) => {
  await environment.newSession();
  const gui = await environment.openGui();
  await gui.setPlatform("Linux");
  await gui.click(".sidebar .row", "New session");
  await gui.sendPrompt("tools");
  await gui.waitForText(".block.permission .shortcut", "Ctrl+Y");
  await gui.waitForText(".block.permission .shortcut", "Ctrl+Alt+Z");
  await gui.pressShortcut("KeyY", { ctrl: true });
  await gui.pressShortcut("KeyZ", { ctrl: true, alt: true });
  await gui.waitForText(".block.agent", "tally-1: go, tally-2: stop");
});

e2eTest("a session that finishes a turn while not shown is unread until it is shown", async (environment) => {
  const shown = await environment.newSession();
  const hidden = await environment.ur("new", "home");
  const gui = await environment.openGui();
  // `title` names the session, so its row can be told apart.
  await environment.ur("prompt", shown, "title");
  await gui.click(".sidebar .row", "tallies");
  await gui.waitForNone(".sidebar .row.unread");
  await environment.ur("prompt", hidden, "hello");
  await gui.waitForText(".sidebar .row.unread", "New session");
  await gui.focusWindow();
  await gui.click(".sidebar .row.unread", "New session");
  await gui.waitForNone(".sidebar .row.unread");
});

e2eTest("a failed turn shows its error and the failed mark", async (environment) => {
  await environment.newSession();
  const gui = await environment.openGui();
  await gui.click(".sidebar .row", "New session");
  await gui.sendPrompt("fail");
  await gui.waitForText(".block.error", "the fake server failed");
  await gui.waitForText(".sidebar .row:has(.status.failed)", "New session");
});

e2eTest("a rejected prompt shows its error under the user message", async (environment) => {
  await environment.newSession();
  const gui = await environment.openGui();
  await gui.click(".sidebar .row", "New session");
  await gui.sendPrompt("reject");
  await gui.waitForText(".block.error", "the fake server rejects this prompt");
  const blocks = await gui.texts(".thread > .block");
  assert.equal(blocks.length, 2, `blocks:\n${blocks.join("\n")}`);
  assert.equal(blocks[0], "reject");
});
