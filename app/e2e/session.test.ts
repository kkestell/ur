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
