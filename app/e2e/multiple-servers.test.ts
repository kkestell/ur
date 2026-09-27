import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { TestEnvironment, e2eTest, waitFor } from "./harness.ts";
import { test } from "node:test";

e2eTest("workspace menu creates a session on the chosen server", async (environment) => {
  const beta = await environment.addServer("Beta");
  const gui = await environment.openGui();
  await gui.contextMenu(".workspace-name");
  assert.deepEqual((await gui.menuEntries()).map((item) => item.text),
    ["New Session — Test", "New Session — Beta", "New Terminal", "Color", "Remove Workspace…"]);
  await gui.chooseNextMenu("New Session — Beta");
  await gui.contextMenu(".workspace-name");
  await waitFor("a Beta session", async () => (await environment.watch()).sessions.some((session) => session.session.includes(beta)));
  await gui.waitForText(".workspace-sessions .row", "Beta");
  await gui.waitForText(".dv-tab", "New session");
  assert.match((await gui.attributes(".tab .label", "title"))[0] ?? "", /Beta/);
});

e2eTest("session rows name their server only when several are configured", async (environment) => {
  await environment.newSession();
  const gui = await environment.openGui();
  await gui.waitForText(".workspace-sessions .row", "New session");
  assert.equal(await gui.hasElement(".workspace-sessions .row .server"), false);
  assert.deepEqual(await gui.attributes(".workspace-sessions .row", "title"), ["New session"]);
  await environment.addServer("Beta");
  await gui.waitForText(".workspace-sessions .row .server", "Test");
  assert.deepEqual(await gui.attributes(".workspace-sessions .row", "title"), ["New session · Test"]);
});

e2eTest("pane menu creates the chosen server's session in its pane", async (environment) => {
  const beta = await environment.addServer("Beta");
  const gui = await environment.openGui();
  await gui.showTerminal();
  await gui.click(".pane-actions button");
  assert.deepEqual((await gui.menuEntries()).map((item) => item.text),
    ["New Session — Test", "New Session — Beta", "New Terminal"]);
  await gui.chooseNextMenu("New Session — Beta");
  await gui.click(".pane-actions button");
  await waitFor("the new session in the pane", async () => (await gui.panes()).some((pane) => pane.includes("New session")));
  assert.ok((await environment.watch()).sessions.some((session) => session.session.includes(beta)));
});

e2eTest("the session shortcut offers every server when several are configured", async (environment) => {
  const beta = await environment.addServer("Beta");
  const gui = await environment.openGui();
  await gui.showTerminal();
  await gui.chooseNextMenu("New Session — Beta");
  await gui.pressShortcut("KeyN");
  await waitFor("the chosen session", async () => (await environment.watch()).sessions.some((session) => session.session.includes(beta)));
  await gui.waitForText(".dv-tab", "New session");
  await gui.setPlatform("Linux");
  await gui.chooseNextMenu("New Session — Test");
  await gui.pressShortcut("KeyN", { ctrl: true });
  await waitFor("the Ctrl shortcut session", async () => (await environment.watch()).sessions.length === 2);
});

test("with no servers the session shortcut opens settings after dismissal", async () => {
  const environment = await TestEnvironment.startManaged();
  try {
    const gui = await environment.openGui(false);
    await gui.waitForText(".server-setup", "ACP servers");
    await gui.click(".server-setup button", "Close");
    await gui.waitForNone(".server-setup");
    await gui.pressShortcut("KeyN");
    await gui.waitForText(".server-setup", "ACP servers");
  } finally { await environment.stop(); }
});

e2eTest("overlapping ACP session IDs keep their transcripts across restarts", async (environment) => {
  const beta = await environment.addServer("Beta");
  const first = await environment.newSession();
  const second = await environment.newSession(beta);
  assert.notEqual(first, second);
  await environment.prompt(first, "hello");
  await environment.prompt(second, "beta");
  await environment.waitForIdle(first);
  await environment.waitForIdle(second);
  let gui = await environment.openGui();
  await gui.click(".workspace-sessions .row", "Test");
  await gui.waitForText(".block.agent", "you said: hello");
  await gui.click(".workspace-sessions .row", "Beta");
  await gui.waitForText(".block.agent", "you said: beta");
  await gui.close();
  await environment.stopDaemon();
  await environment.startDaemon();
  gui = await environment.openGui();
  await gui.waitForText(".block.agent", "you said: beta");
  await gui.click(".workspace-sessions .row", "Test");
  await gui.waitForText(".block.agent", "you said: hello");
});

e2eTest("a server's icon marks its session rows and tabs, and settings change it", async (environment) => {
  await environment.newSession();
  const gui = await environment.openGui();
  await gui.click(".workspace-sessions .row", "New session");
  await gui.waitForText(".dv-tab", "New session");
  assert.deepEqual(await gui.attributes(".workspace-sessions .row .server-icon", "data-icon"), ["claude"]);
  assert.deepEqual(await gui.attributes(".tab .server-icon", "data-icon"), ["claude"]);

  await gui.click(".sidebar-header [title='Server Settings']");
  await gui.click(".server-setup .icon-choice[title='Codex']");
  await gui.click(".server-save");
  await gui.waitForNone(".server-setup");
  await waitFor("the tab's new icon", async () => (await gui.attributes(".tab .server-icon", "data-icon"))[0] === "codex");
  assert.deepEqual(await gui.attributes(".workspace-sessions .row .server-icon", "data-icon"), ["codex"]);
  assert.equal(JSON.parse(readFileSync(environment.configFile, "utf8")).servers[0].icon, "codex");
});

e2eTest("settings add rename and remove a server without changing another", async (environment) => {
  const gui = await environment.openGui();
  await gui.click(".sidebar-header [title='Server Settings']");
  await gui.click(".server-setup button", "Add Server");
  await gui.setInput(".server-name", "Beta");
  await gui.setInput(".server-command", "/no/such/server");
  await gui.click(".server-setup button", "Add argument");
  await gui.setInput(".server-argument", environment.home + "/beta-history.json");
  await gui.click(".server-save");
  await gui.waitForText(".server-error", "/no/such/server");
  await gui.setInput(".server-command", environment.fakeServer);
  await gui.click(".server-save");
  await waitFor("Beta to connect", async () => (await environment.watch()).servers.some((server) => server.name === "Beta" && server.connected));
  await gui.waitForNone(".server-setup");
  const beta = (await environment.watch()).servers.find((server) => server.name === "Beta")!.id;
  const session = await environment.newSession(beta);
  await gui.click(".workspace-sessions .row", "Beta");
  await gui.waitForText(".dv-tab", "New session");
  await gui.click(".sidebar-header [title='Server Settings']");
  await gui.click(".server-setup button", "Beta");
  await gui.setInput(".server-name", "Renamed");
  await gui.click(".server-save");
  await waitFor("the rename", async () => (await environment.watch()).servers.some((server) => server.id === beta && server.name === "Renamed"));
  assert.equal((await environment.watch()).servers[0].name, "Test");
  await gui.click(".sidebar-header [title='Server Settings']");
  await gui.click(".server-setup button", "Renamed");
  await gui.confirmNextDialog();
  await gui.click(".server-remove");
  assert.match((await gui.lastDialogQuestion()) ?? "", /running sessions in ur will stop/);
  await waitFor("Beta to be removed", async () => !(await environment.watch()).servers.some((server) => server.id === beta));
  assert.equal((await environment.watch()).sessions.some((item) => item.session === session), false);
  await gui.waitForNone(".dv-tab");
});

e2eTest("a session uses its server's image and delete capabilities", async (environment) => {
  const beta = await environment.addServer("Beta", undefined, ["--no-image", "--no-delete"]);
  const first = await environment.newSession();
  const second = await environment.newSession(beta);
  const gui = await environment.openGui();
  await gui.click(".workspace-sessions .row", "Beta");
  await gui.dropFile(".editor", "image.png", "image/png", Buffer.from("image data").toString("base64"));
  await gui.waitForText(".editor-message", "does not accept images");
  await gui.contextMenu(".workspace-sessions .row:nth-child(1)");
  assert.deepEqual(await gui.menuEntries(), []);
  await gui.click(".workspace-sessions .row", "Test");
  await gui.dropFile(".editor", "image.png", "image/png", Buffer.from("image data").toString("base64"));
  await gui.waitForText(".chip", "image.png");
  await gui.contextMenu(".workspace-sessions .row:nth-child(2)");
  await waitFor("the Delete menu", async () => (await gui.menuEntries()).some((item) => item.text === "Delete…"));
  assert.notEqual(first, second);
});

e2eTest("another server and a terminal continue while one server is unavailable", async (environment) => {
  const beta = await environment.addServer("Beta");
  const first = await environment.newSession();
  await environment.newSession(beta);
  const gui = await environment.openGui();
  await gui.showTerminal();
  await environment.request({ type: "update_server", server: beta, name: "Beta", icon: "claude", command: "/no/such/server", args: [] });
  await gui.waitForText(".server-banner", "Beta");
  await gui.contextMenu(".workspace-name");
  assert.deepEqual((await gui.menuEntries()).filter((item) => item.text.includes("Beta")),
    [{ text: "New Session — Beta", enabled: false }]);
  await environment.prompt(first, "still working");
  await environment.waitForIdle(first);
  await gui.click(".workspace-sessions .row", "Test");
  await gui.waitForText(".block.agent", "you said: still working");
  await gui.showTerminal();
  await gui.type("echo terminal still works\r");
  await gui.waitForLine("terminal still works");
});
