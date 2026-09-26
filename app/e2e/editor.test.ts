import assert from "node:assert/strict";
import { type Gui, type TestEnvironment, e2eTest, waitFor } from "./harness.ts";

// A 1×1 PNG.
const PNG =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

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
  await gui.sendPrompt("look");
  await gui.waitForText(".block.agent", "you said: look (1 image)");
  await gui.waitForNone(".attachments");
  assert.ok(await gui.hasElement(".block.user .thumbnails img"), "no thumbnail");
});

e2eTest("a dropped file that is not an image is refused", async (environment) => {
  const gui = await openSession(environment);
  await gui.dropFile(".editor", "notes.txt", "text/plain", btoa("notes"));
  await gui.waitForText(".editor-message", "notes.txt is not an image.");
  await gui.waitForNone(".attachments");
});
