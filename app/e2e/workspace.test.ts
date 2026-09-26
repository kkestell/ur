import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { e2eTest } from "./harness.ts";

e2eTest("adding a workspace asks for its workspace color", async (environment) => {
  const folder = join(environment.home, "project");
  mkdirSync(folder);
  const gui = await environment.openGui();

  await gui.chooseFolder(folder);
  await gui.click(".sidebar-header [title='Add Workspace']");
  await gui.waitForText(".workspace-color-picker", "project");
  assert.equal((await gui.texts(".workspace-color-picker .swatch")).length, 14);
  await gui.pressKey(".workspace-color-picker", "Escape");
  await gui.waitForNone(".workspace-color-picker");
  assert.deepEqual(await gui.texts(".workspace-name .label"), ["home"]);

  await gui.chooseFolder(folder);
  await gui.click(".sidebar-header [title='Add Workspace']");
  await gui.click(".workspace-color-picker .swatch[title='Mauve']");
  await gui.waitForNone(".workspace-color-picker");
  await gui.waitForText(".workspace-name .label", "project");
});
