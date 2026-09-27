import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { e2eTest, waitFor } from "./harness.ts";

e2eTest("the Add Workspace dialog adds the chosen folder in the chosen color", async (environment) => {
  const folder = join(environment.home, "project");
  mkdirSync(folder);
  const gui = await environment.openGui();

  await gui.click(".sidebar-header [title='Add Workspace']");
  await gui.chooseFolder(folder);
  await gui.click(".add-workspace .workspace-choose");
  await gui.click(".add-workspace .swatch[title='Mauve']");
  await gui.click(".add-workspace .workspace-cancel");
  await gui.waitForNone(".add-workspace");
  assert.deepEqual(await gui.texts(".workspace-name .label"), ["home"]);

  await gui.click(".sidebar-header [title='Add Workspace']");
  await gui.chooseFolder(folder);
  await gui.click(".add-workspace .workspace-choose");
  await waitFor("the chosen folder", async () => (await gui.inputValue(".add-workspace .workspace-path")) === folder);
  assert.equal(await gui.hasElement(".add-workspace .workspace-add:disabled"), true);
  await gui.click(".add-workspace .swatch[title='Mauve']");
  await gui.click(".add-workspace .workspace-add");
  await gui.waitForNone(".add-workspace");
  await gui.waitForText(".workspace-name .label", "project");
  const workspace = (await environment.watch()).workspaces.find((workspace) => workspace.name === "project");
  assert.deepEqual(workspace, { name: "project", path: folder, color: "mauve" });
});

e2eTest("the Add Workspace dialog shows a rejected path and stays open", async (environment) => {
  const gui = await environment.openGui();

  await gui.click(".sidebar-header [title='Add Workspace']");
  await gui.setInput(".add-workspace .workspace-path", join(environment.home, "missing"));
  await gui.click(".add-workspace .swatch[title='Green']");
  await gui.click(".add-workspace .workspace-add");
  await gui.waitForText(".add-workspace .workspace-error", "is not a directory");
  assert.deepEqual(await gui.texts(".workspace-name .label"), ["home"]);
});

e2eTest("the window's shortcuts do nothing while a dialog is open", async (environment) => {
  const gui = await environment.openGui();

  await gui.click(".sidebar-header [title='Add Workspace']");
  await gui.pressShortcut("KeyN", { shift: true, target: ".add-workspace .workspace-path" });
  await gui.click(".add-workspace .workspace-cancel");
  await gui.waitForNone(".add-workspace");
  await gui.pressShortcut("KeyN", { shift: true });
  await waitFor("the terminal tab", async () => (await gui.displayedCount(".tab .terminal-icon")) > 0);
  assert.equal(await gui.displayedCount(".tab .terminal-icon"), 1);
});
