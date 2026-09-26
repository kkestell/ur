import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { TestEnvironment, e2eTest, waitFor } from "./harness.ts";

test("the app starts its daemon and saves a working server after a failed choice", async () => {
  const environment = await TestEnvironment.startManaged();
  try {
    let gui = await environment.openGui(false);
    await gui.waitForText(".server-setup", "Connect an ACP server");
    await waitFor("the bundled daemon to listen", async () => {
      try { await environment.watch(); return true; } catch { return false; }
    });
    assert.equal((await environment.watch()).server_state.error, null);
    assert.equal(await gui.hasElement(".server-error"), false);

    await gui.setInput(".server-command", "/no/such/server");
    await gui.click(".server-save");
    await gui.waitForText(".server-error", "/no/such/server");

    await gui.setInput(".server-command", environment.fakeServer);
    await gui.click(".server-setup button", "Add argument");
    await gui.setInput(".server-argument", environment.historyFile);
    await gui.click(".server-save");
    await waitFor("the server to connect", async () => (await environment.watch()).server_state.connected);
    assert.deepEqual(JSON.parse(readFileSync(environment.configFile, "utf8")), {
      server: { command: environment.fakeServer, args: [environment.historyFile] },
    });
    await environment.addWorkspace("home", environment.home);
    await gui.waitForText(".workspace-name", "home");
    const session = await environment.newSession();
    await gui.click(".sidebar .row", "New session");
    await gui.sendPrompt("hello");
    await gui.waitForText(".block.agent", "you said: hello");
    await gui.close();

    gui = await environment.openGui();
    await gui.waitForText(".block.agent", "you said: hello");
    assert.equal((await environment.watch()).server_state.connected, true);
    assert.ok(session);
  } finally {
    await environment.stop();
  }
});

e2eTest("terminals stay available while the configured server is unavailable", async (environment) => {
  const gui = await environment.openGui();
  await gui.showTerminal();
  await gui.click(".sidebar-header button", "Server");
  await gui.setInput(".server-command", "/no/such/server");
  await gui.click(".server-save");
  await gui.waitForText(".server-banner", "/no/such/server");
  await gui.type("echo terminal still works\r");
  await gui.waitForLine("terminal still works");
});
