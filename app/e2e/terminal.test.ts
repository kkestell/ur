import assert from "node:assert/strict";
import { existsSync, mkdirSync, realpathSync } from "node:fs";
import { join } from "node:path";
import { e2eTest, waitFor } from "./harness.ts";

const ESCAPE = "\x1b";

e2eTest("a terminal survives closing and reopening the GUI", async (environment) => {
  let gui = await environment.openGui();
  await gui.showTerminal();
  const defaultRows = (await gui.lines()).length;
  await gui.setWindowSize(700, 450);
  await waitFor("the view to shrink", async () => (await gui.lines()).length < defaultRows);
  const smallRows = (await gui.lines()).length;
  await gui.type("vi notes.txt\r");
  await gui.type(`ifirst line\rsecond line${ESCAPE}`);
  await gui.waitForLine("second line");
  await gui.close();

  // Reopening at the default size attaches at a different size than before.
  gui = await environment.openGui();
  await gui.showTerminal();
  await gui.waitForLine("first line");
  await gui.waitForLine("second line");
  const rows = (await gui.lines()).length;
  assert.notEqual(rows, smallRows, "the GUI reopened at the size it closed at");
  await gui.type(":set lines?\r");
  await gui.waitForLine(`lines=${rows}`);
  await gui.type(`Gothird line${ESCAPE}`);
  await gui.waitForLine("third line");
  assert.equal(existsSync(join(environment.home, "notes.txt")), false, "notes.txt was saved");
});

e2eTest("quitting a restored full-screen application returns to the shell", async (environment) => {
  let gui = await environment.openGui();
  await gui.showTerminal();
  await gui.type("top\r");
  await gui.waitForLine(/^Processes:/);
  await gui.close();

  gui = await environment.openGui();
  await gui.showTerminal();
  await gui.type("q");
  await gui.type("echo back at the prompt\r");
  await gui.waitForLine("back at the prompt");
  const lines = await gui.lines();
  assert.ok(
    !lines.some((row) => row.startsWith("Processes:")),
    `top's last frame is still on screen:\n${lines.join("\n")}`,
  );
});

e2eTest("a terminal's row and tab show the shell's name until a program sets a title", async (environment) => {
  const gui = await environment.openGui();
  await gui.showTerminal();
  await gui.waitForText(".sidebar .row:has(.terminal-icon) .label", "sh");
  await gui.waitForText(".tab .label", "sh");
  await gui.type("printf '\\033]0;tallying\\007'\r");
  await gui.waitForText(".sidebar .row:has(.terminal-icon) .label", "tallying");
  await gui.waitForText(".tab .label", "tallying");
});

e2eTest("a terminal starts in its workspace's directory", async (environment) => {
  const other = join(environment.home, "other");
  mkdirSync(other);
  await environment.ur("workspace", "add", "other", other);
  const gui = await environment.openGui();
  // WebDriver cannot drive the native workspace menu.
  await gui.request({ type: "open_terminal", workspace: "other" });
  await gui.click(".sidebar .row:has(.terminal-icon)");
  const workspaces = await gui.texts(".workspace:has(.terminal-icon) .workspace-name .label");
  assert.deepEqual(workspaces, ["other"]);
  await gui.type("pwd\r");
  await gui.waitForLine(realpathSync(other));
});

e2eTest("Close Terminal removes the terminal's row", async (environment) => {
  const gui = await environment.openGui();
  await gui.showTerminal();
  const opened = (await gui.request({ type: "open_terminal", workspace: "home" })) as {
    terminal: number;
  };
  const rows = async () => (await gui.texts(".sidebar .row:has(.terminal-icon)")).length;
  await waitFor("two terminal rows", async () => (await rows()) === 2);
  // Close Terminal is a native menu, which WebDriver cannot drive.
  await gui.request({ type: "close_terminal", terminal: opened.terminal });
  await waitFor("one terminal row", async () => (await rows()) === 1);
  await gui.type("echo still here\r");
  await gui.waitForLine("still here");
});

e2eTest("a terminal whose shell exits leaves the sidebar and closes its tab", async (environment) => {
  const gui = await environment.openGui();
  await gui.showTerminal();
  await gui.type("exit\r");
  await gui.waitForNone(".sidebar .row:has(.terminal-icon)");
  await gui.waitForNone(".tab");
  await gui.waitForText(".empty", "Select a session");
});

e2eTest("removing a workspace stops its terminals", async (environment) => {
  const gui = await environment.openGui();
  await gui.showTerminal();
  await environment.ur("workspace", "rm", "home");
  await gui.waitForNone(".sidebar .row:has(.terminal-icon)");
  await gui.waitForText(".empty", "No workspaces");
});
