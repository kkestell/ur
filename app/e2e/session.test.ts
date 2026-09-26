import assert from "node:assert/strict";
import { setTimeout as sleep } from "node:timers/promises";
import { e2eTest } from "./harness.ts";

e2eTest("the empty states follow the workspaces and the selection", async (environment) => {
  const gui = await environment.openGui();
  await gui.waitForText(".empty", "Select a session");
  await environment.ur("workspace", "rm", "home");
  await gui.waitForText(".empty", "No workspaces");
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
  assert.deepEqual(await gui.texts(".block.agent p"), ["you said: hello"]);

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
  await gui.waitForNone(".workspace .row");
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

e2eTest("the session header shows the session title", async (environment) => {
  const session = await environment.newSession();
  const gui = await environment.openGui();
  await gui.click(".sidebar .row", "New session");
  await gui.waitForText(".session-header .label", "New session");
  await environment.ur("prompt", session, "title");
  await gui.waitForText(".session-header .label", "tallies");
  await gui.waitForText(".sidebar .row", "tallies");
});

e2eTest("the session header's + creates a session in its workspace and selects it", async (environment) => {
  const session = await environment.newSession();
  await environment.ur("prompt", session, "title");
  const gui = await environment.openGui();
  await gui.click(".sidebar .row", "tallies");
  await gui.click(".session-header .icon-button", "+");
  await gui.waitForText(".sidebar .row.selected", "New session");
  await gui.waitForText(".session-header .label", "New session");
  assert.equal((await gui.texts(".workspace .row")).length, 2);
});

e2eTest("deleting a session removes it from the sidebar", async (environment) => {
  const session = await environment.newSession();
  const gui = await environment.openGui();
  await gui.click(".sidebar .row", "New session");
  // Delete… is a native menu and confirmation, which WebDriver cannot drive.
  await gui.request({ type: "delete_session", session });
  await gui.waitForNone(".workspace .row");
  await gui.waitForText(".empty", "Select a session");
});

e2eTest("a session holding 20 MB of images loads after reopening the GUI", async (environment) => {
  const session = await environment.newSession();
  let gui = await environment.openGui();
  await gui.click(".sidebar .row", "New session");
  await gui.promptWithImage(session, "big", 20 * 1024 * 1024);
  await gui.waitForText(".block.agent", "you said: big (1 image)");
  await gui.close();

  // The session snapshot holds the image, in one frame over 16 MiB.
  gui = await environment.openGui();
  await gui.waitForText(".block.agent", "you said: big (1 image)");
  await sleep(1000);
  assert.ok(!(await gui.hasElement(".connecting")), "the GUI lost its connection");
  await gui.sendPrompt("after");
  await gui.waitForText(".block.agent", "you said: after");
});
