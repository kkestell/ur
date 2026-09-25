import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { join } from "node:path";
import { e2eTest } from "./harness.ts";

const ESCAPE = "\x1b";

e2eTest("a terminal survives closing and reopening the GUI", async (environment) => {
  let gui = await environment.openGui();
  await gui.setWindowSize(700, 450);
  await gui.type("vi notes.txt\r");
  await gui.type(`ifirst line\rsecond line${ESCAPE}`);
  await gui.waitForLine("second line");
  await gui.close();

  // Reopening at the default size attaches at a different size than before.
  gui = await environment.openGui();
  await gui.waitForLine("first line");
  await gui.waitForLine("second line");
  const rows = (await gui.lines()).length;
  await gui.type(":set lines?\r");
  await gui.waitForLine(`lines=${rows}`);
  await gui.type(`Gothird line${ESCAPE}`);
  await gui.waitForLine("third line");
  assert.equal(existsSync(join(environment.home, "notes.txt")), false, "notes.txt was saved");
});

e2eTest("quitting a restored full-screen application returns to the shell", async (environment) => {
  let gui = await environment.openGui();
  await gui.type("top\r");
  await gui.waitForLine(/^Processes:/);
  await gui.close();

  gui = await environment.openGui();
  await gui.type("q");
  await gui.type("echo back at the prompt\r");
  await gui.waitForLine("back at the prompt");
  const lines = await gui.lines();
  assert.ok(
    !lines.some((row) => row.startsWith("Processes:")),
    `top's last frame is still on screen:\n${lines.join("\n")}`,
  );
});
