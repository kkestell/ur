import assert from "node:assert/strict";
import { type Gui, type TestEnvironment, e2eTest, waitFor } from "./harness.ts";

// A 1×1 PNG.
const PNG =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
const OTHER_PNG = Buffer.concat([Buffer.from(PNG, "base64"), Buffer.from([0])]).toString("base64");

/** Opens the GUI on a new session. */
async function openSession(environment: TestEnvironment): Promise<Gui> {
  await environment.newSession();
  const gui = await environment.openGui();
  await gui.click(".sidebar .row", "New session");
  return gui;
}

e2eTest("typing / lists the server's slash commands, and Enter inserts one", async (environment) => {
  const gui = await openSession(environment);
  await gui.typePrompt("/");
  await gui.waitForText(".command-list .command", "/tally");
  await gui.waitForText(".command-list .command", "count the tallies");
  await gui.pressKey(".editor textarea", "Enter");
  await gui.waitForNone(".command-list");
  assert.equal(await gui.promptText(), "/tally");
});

e2eTest("choosing a config option value sets it on the server", async (environment) => {
  const gui = await openSession(environment);
  await gui.click(".picker", "Steady");
  await gui.click(".picker-value", "Brisk");
  await gui.waitForText(".picker", "Brisk");
  // Reopening the GUI loads the value the server holds.
  await gui.close();
  const reopened = await environment.openGui();
  await reopened.waitForText(".picker", "Brisk");
});

e2eTest("a press outside a config picker closes it", async (environment) => {
  const gui = await openSession(environment);
  await gui.waitForText(".picker", "Steady");
  await gui.press(".picker", "Steady");
  await gui.waitForText(".picker-list .picker-value", "Brisk");
  await gui.press(".editor textarea");
  await gui.waitForNone(".picker-list");
});

e2eTest("a config option the server changes updates its picker", async (environment) => {
  const gui = await openSession(environment);
  await gui.waitForText(".picker", "Steady");
  await gui.sendPrompt("pace");
  await gui.waitForText(".picker", "Brisk");
});

e2eTest("config pickers that do not fit the editor's row move to the More menu", async (environment) => {
  const session = await environment.newSession();
  const gui = await environment.openGui();
  await gui.setWindowSize(900, 600);
  await gui.showTerminal();
  await gui.click(".sidebar .row", "New session");
  await gui.dragTab("New session", 0, "right");
  await environment.ur("prompt", session, "options");
  await waitFor("the More button", async () => (await gui.displayedCount(".more-options")) === 1);
  assert.ok(await gui.editorControlsFit(), "the editor controls wrap or overflow");

  const inline = await gui.texts(".editor-options .picker");
  await gui.click(".more-options button");
  await gui.waitForText(".more-options-list .picker", "");
  assert.deepEqual(
    [...inline, ...(await gui.texts(".more-options-list .picker"))],
    ["DeepSeek: DeepSeek Reasoner", "Steady", "Auto"],
  );
  assert.ok(await gui.insideWindow(".more-options-list"), "the More menu is cut off");

  await gui.setWindowSize(1600, 900);
  await gui.waitForNone(".more-options");
  assert.deepEqual(await gui.texts(".editor-options .picker"), [
    "DeepSeek: DeepSeek Reasoner",
    "Steady",
    "Auto",
  ]);
});

e2eTest("a model that accepts images shows the image icon in its list", async (environment) => {
  const gui = await openSession(environment);
  await gui.setWindowSize(1600, 900);
  await gui.sendPrompt("options");
  await gui.click(".picker", "DeepSeek");
  await gui.waitForText(".picker-value", "Google: Gemma Vision");
  assert.deepEqual(await gui.texts(".picker-value"), [
    "DeepSeek: DeepSeek Reasoner",
    "Google: Gemma Vision",
  ]);
  assert.deepEqual(await gui.texts(".picker-value:has(.accepts-images)"), [
    "Google: Gemma Vision",
  ]);
});

e2eTest("usage the server reports shows in the usage indicator", async (environment) => {
  const gui = await openSession(environment);
  await gui.waitForNone(".usage");
  await gui.sendPrompt("usage");
  await gui.hover(".usage");
  await gui.waitForText(".usage-popover", "1.2k / 8k tokens (15%)");
  await gui.waitForText(".usage-popover", "Cost $0.25");
});

e2eTest("an image dropped on the editor is sent with the prompt", async (environment) => {
  const gui = await openSession(environment);
  await gui.dropFile(".editor", "dot.png", "image/png", PNG);
  await gui.waitForText(".attachments .chip", "dot.png");
  await gui.waitForNone(".attachments .loading");
  await gui.sendPrompt("look");
  await gui.waitForText(".block.agent", "you said: look (1 image)");
  await gui.waitForNone(".attachments");
  assert.ok(await gui.hasElement(".block.user .thumbnails img"), "no thumbnail");
});

e2eTest("a pending image blocks sending until it is read or removed", async (environment) => {
  const gui = await openSession(environment);
  await gui.holdFileReads();
  await gui.dropFile(".editor", "dot.png", "image/png", PNG);
  await gui.waitForText(".attachments .chip", "dot.png");
  await gui.sendPrompt("look");
  assert.ok(await gui.hasElement(".editor-actions button:disabled"), "Send is enabled during a read");
  assert.equal(await gui.promptText(), "look");
  assert.equal(await gui.hasElement(".block.user"), false, "prompt sent during a read");

  await gui.releaseFileRead(0);
  await gui.waitForNone(".attachments .loading");
  await gui.pressKey(".editor textarea", "Enter");
  await gui.waitForText(".block.agent", "you said: look (1 image)");
  assert.deepEqual(await gui.attributes(".block.user .thumbnails img", "src"), [
    `data:image/png;base64,${PNG}`,
  ]);

  await gui.dropFile(".editor", "removed.png", "image/png", OTHER_PNG);
  await gui.waitForText(".attachments .chip", "removed.png");
  await gui.click(".attachments .chip .remove");
  await gui.releaseFileRead(1);
  await gui.waitForNone(".attachments");
  await gui.sendPrompt("without");
  await gui.waitForText(".block.agent", "you said: without");
  assert.deepEqual(await gui.attributes(".block.user .thumbnails img", "src"), [
    `data:image/png;base64,${PNG}`,
  ]);
});

e2eTest("images dropped together reach the prompt in drop order", async (environment) => {
  const gui = await openSession(environment);
  await gui.holdFileReads();
  await gui.dropFiles(".editor", [
    { name: "first.png", type: "image/png", data: PNG },
    { name: "second.png", type: "image/png", data: OTHER_PNG },
  ]);
  await gui.releaseFileRead(1);
  await gui.releaseFileRead(0);
  await gui.waitForNone(".attachments .loading");
  assert.deepEqual(await gui.texts(".attachments .chip .label"), ["first.png", "second.png"]);
  await gui.sendPrompt("order");
  await gui.waitForText(".block.agent", "you said: order (2 images)");
  assert.deepEqual(await gui.attributes(".block.user .thumbnails img", "src"), [
    `data:image/png;base64,${PNG}`,
    `data:image/png;base64,${OTHER_PNG}`,
  ]);
});

e2eTest("a dropped file that is not an image is refused", async (environment) => {
  const gui = await openSession(environment);
  await gui.dropFile(".editor", "notes.txt", "text/plain", btoa("notes"));
  await gui.waitForText(".editor-message", "notes.txt is not an image.");
  await gui.waitForNone(".attachments");
});
