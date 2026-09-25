import assert from "node:assert/strict";
import { e2eTest } from "./harness.ts";

e2eTest("the empty states follow the workspaces and the selection", async (environment) => {
  const gui = await environment.openGui();
  await gui.waitForText(".empty", "No workspaces");
  await environment.newSession();
  await gui.waitForText(".empty", "Select a session");
});

e2eTest("a session created from the CLI answers a prompt sent from the editor", async (environment) => {
  await environment.newSession();
  const gui = await environment.openGui();
  await gui.click(".sidebar .row", "New session");
  await gui.sendPrompt("hello");
  await gui.waitForText(".block.user", "hello");
  await gui.waitForText(".block.agent", "you said: hello");
});

e2eTest("Stop cancels a running turn", async (environment) => {
  await environment.newSession();
  const gui = await environment.openGui();
  await gui.click(".sidebar .row", "New session");
  // `tool` waits for a permission answer, which Stop cancels.
  await gui.sendPrompt("tool");
  await gui.click(".editor-actions button", "Stop");
  await gui.waitForText(".editor-actions button", "Send");
});

e2eTest("the selected session and its transcript survive closing and reopening the GUI", async (environment) => {
  await environment.newSession();
  let gui = await environment.openGui();
  await gui.click(".sidebar .row", "New session");
  await gui.sendPrompt("hello");
  await gui.waitForText(".block.agent", "you said: hello");
  await gui.close();

  gui = await environment.openGui();
  await gui.waitForText(".block.agent", "you said: hello");
  await gui.waitForText(".sidebar .row.selected", "New session");
});

e2eTest("the GUI reconnects after a daemon restart without duplicating the thread", async (environment) => {
  await environment.newSession();
  const gui = await environment.openGui();
  await gui.click(".sidebar .row", "New session");
  await gui.sendPrompt("hello");
  await gui.waitForText(".block.agent", "you said: hello");

  await environment.stopDaemon();
  await gui.waitForText(".connecting", "Connecting to");
  await environment.startDaemon();
  await gui.waitForText(".block.agent", "you said: hello");
  assert.deepEqual(await gui.texts(".block.user"), ["hello"]);
  assert.deepEqual(await gui.texts(".block.agent"), ["you said: hello"]);

  await gui.sendPrompt("again");
  await gui.waitForText(".block.agent", "you said: again");
});

e2eTest("a prompt the daemon answers busy comes back to the editor", async (environment) => {
  await environment.newSession();
  const gui = await environment.openGui();
  await gui.click(".sidebar .row", "New session");
  await gui.sendPrompt("tool");
  await gui.waitForText(".editor-actions button", "Stop");
  await gui.sendPrompt("second");
  await gui.waitForText(".editor-message", "The session is busy.");
  assert.equal(await gui.promptText(), "second");
});

e2eTest("an editor draft does not follow the selection to another session", async (environment) => {
  await environment.newSession();
  await environment.ur("new", "home");
  const gui = await environment.openGui();
  await gui.click(".sidebar .row:nth-child(1)");
  await gui.typePrompt("first draft");
  await gui.click(".sidebar .row:nth-child(2)");
  await gui.waitForText(".sidebar .row.selected:nth-child(2)", "New session");
  assert.equal(await gui.promptText(), "");
});

e2eTest("a session that comes back with its workspace shows its thread", async (environment) => {
  await environment.newSession();
  const gui = await environment.openGui();
  await gui.click(".sidebar .row", "New session");
  await gui.sendPrompt("hello");
  await gui.waitForText(".block.agent", "you said: hello");

  await environment.ur("workspace", "rm", "home");
  await gui.waitForNone(".sidebar .row:not(.terminal-row)");
  await environment.ur("workspace", "add", "home", environment.home);
  await gui.click(".sidebar .row", "New session");
  await gui.waitForText(".block.agent", "you said: hello");
});

e2eTest("another session's activity leaves the thread's scroll position alone", async (environment) => {
  await environment.newSession();
  const other = await environment.ur("new", "home");
  const gui = await environment.openGui();
  await gui.setWindowSize(700, 400);
  await gui.click(".sidebar .row:nth-child(1)");
  for (let prompt = 1; prompt <= 8; prompt++) {
    await gui.sendPrompt(`prompt ${prompt}`);
    await gui.waitForText(".block.agent", `you said: prompt ${prompt}`);
  }
  await gui.scrollThread(0);
  await environment.ur("prompt", other, "hello");
  await environment.ur("wait", other);
  assert.equal(await gui.threadScrollTop(), 0);
});
