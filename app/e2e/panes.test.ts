import assert from "node:assert/strict";
import { type Gui, type TestEnvironment, e2eTest, waitFor } from "./harness.ts";

/** Waits until the panes and their tabs are `expected`, as `Gui.panes()` gives them. */
async function waitForPanes(gui: Gui, expected: string[]): Promise<void> {
  let panes: string[] = [];
  try {
    await waitFor(`the panes ${expected.join(" ")}`, async () => {
      panes = await gui.panes();
      return JSON.stringify(panes) === JSON.stringify(expected);
    });
  } catch (error) {
    throw new Error(`the panes are ${panes.join(" ")}`, { cause: error });
  }
}

/**
 * Opens the GUI with the terminal's tab in the left pane and the session
 * named `tallies` in the right one, which is active.
 */
async function twoPanes(environment: TestEnvironment): Promise<{ gui: Gui; session: string }> {
  const session = await environment.newSession();
  await environment.ur("prompt", session, "title");
  const gui = await environment.openGui();
  await gui.showTerminal();
  await gui.click(".sidebar .row", "tallies");
  await waitForPanes(gui, ["*[sh, *tallies]"]);
  await gui.dragTab("tallies", 0, "right");
  await waitForPanes(gui, ["[*sh]", "*[*tallies]"]);
  return { gui, session };
}

e2eTest("dragging a tab to a pane's edge makes a new pane", async (environment) => {
  await twoPanes(environment);
});

e2eTest("tabs move with pointer events, not native drag and drop, and without starting a text selection", async (environment) => {
  const { gui } = await twoPanes(environment);
  assert.deepEqual(await gui.attributes(".dv-tab", "draggable"), ["false", "false"]);
  assert.ok(await gui.mouseDownCanceled(".dv-tab .label"), "the tab press can start a text selection");
});

e2eTest("tabs do not show a hidden-tab counter", async (environment) => {
  for (let index = 0; index < 5; index++) {
    await environment.newSession();
  }
  const gui = await environment.openGui();
  for (let index = 1; index <= 5; index++) {
    await gui.click(`.workspace-sessions .row:nth-child(${index})`);
  }
  assert.equal((await gui.panes())[0].match(/New session/g)?.length, 5);
  await gui.waitForNone(".dv-tabs-overflow-dropdown-default");
});

e2eTest("activating a tab scrolls it fully into view", async (environment) => {
  const count = 16;
  for (let index = 0; index < count; index++) {
    await environment.newSession();
  }
  const gui = await environment.openGui();
  for (let index = 1; index <= count; index++) {
    await gui.click(`.workspace-sessions .row:nth-child(${index})`);
    await waitFor(`tab ${index} to be fully visible`, () => gui.activeTabsVisible());
  }
  await gui.click(".workspace-sessions .row:nth-child(1)");
  await waitFor("the first tab to be fully visible", () => gui.activeTabsVisible());
});

e2eTest("⌘N opens a new session and ⇧⌘N a new terminal in the active pane", async (environment) => {
  const gui = await environment.openGui();
  await gui.pressShortcut("KeyN");
  await waitForPanes(gui, ["*[*New session]"]);
  await gui.pressShortcut("KeyN", { shift: true });
  await waitForPanes(gui, ["*[New session, *sh]"]);
  await gui.waitForText(".sidebar .row:has(.terminal-icon) .label", "sh");
});

e2eTest("Ctrl opens, closes, and reopens tabs on other platforms", async (environment) => {
  const gui = await environment.openGui();
  await gui.setPlatform("Linux");
  await gui.pressShortcut("KeyN", { ctrl: true });
  await waitForPanes(gui, ["*[*New session]"]);
  await gui.pressShortcut("KeyN", { ctrl: true, shift: true });
  await waitForPanes(gui, ["*[New session, *sh]"]);
  await gui.pressShortcut("KeyW", { ctrl: true });
  await waitForPanes(gui, ["*[*New session]"]);
  await gui.pressShortcut("KeyT", { ctrl: true, shift: true });
  await waitForPanes(gui, ["*[New session, *sh]"]);
});

e2eTest("the close shortcut removes only the active tab, even with the editor or terminal focused", async (environment) => {
  const gui = await environment.openGui();
  await gui.showTerminal();
  await gui.pressShortcut("KeyW", { target: ".xterm-helper-textarea" });
  await waitForPanes(gui, []);
  await gui.waitForText(".sidebar .row:has(.terminal-icon)", "sh");
  await gui.pressShortcut("KeyN");
  await waitForPanes(gui, ["*[*New session]"]);
  await gui.pressShortcut("KeyW", { target: ".editor textarea" });
  await waitForPanes(gui, []);
  await gui.waitForText(".sidebar .row", "New session");
});

e2eTest("Ctrl+W remains available to a terminal on macOS", async (environment) => {
  const gui = await environment.openGui();
  await gui.showTerminal();
  await gui.pressShortcut("KeyW", { ctrl: true, target: ".xterm-helper-textarea" });
  await waitForPanes(gui, ["*[*sh]"]);
});

e2eTest("closed tabs reopen newest first in the active pane", async (environment) => {
  const { gui } = await twoPanes(environment);
  await gui.pressShortcut("KeyW");
  await waitForPanes(gui, ["*[*sh]"]);
  await gui.click(".tab .tab-close");
  await waitForPanes(gui, []);
  await gui.pressShortcut("KeyT", { shift: true });
  await waitForPanes(gui, ["*[*sh]"]);
  await gui.pressShortcut("KeyT", { shift: true });
  await waitForPanes(gui, ["*[sh, *tallies]"]);
});

e2eTest("recently closed tabs are forgotten after restarting the GUI", async (environment) => {
  const gui = await environment.openGui();
  await gui.showTerminal();
  await gui.pressShortcut("KeyW");
  await waitForPanes(gui, []);
  await gui.close();
  const reopened = await environment.openGui();
  await reopened.pressShortcut("KeyT", { shift: true });
  await waitForPanes(reopened, []);
});

e2eTest("reopen skips a tab already opened from the sidebar", async (environment) => {
  const gui = await environment.openGui();
  await gui.showTerminal();
  await gui.pressShortcut("KeyW");
  await waitForPanes(gui, []);
  await gui.click(".sidebar .row:has(.terminal-icon)");
  await waitForPanes(gui, ["*[*sh]"]);
  await gui.pressShortcut("KeyT", { shift: true });
  await waitForPanes(gui, ["*[*sh]"]);
});

e2eTest("reopen skips a terminal that has been removed", async (environment) => {
  const gui = await environment.openGui();
  const opened = (await gui.request({ type: "open_terminal", workspace: "home" })) as { terminal: number };
  await gui.click(".sidebar .row:has(.terminal-icon)");
  await waitForPanes(gui, ["*[*sh]"]);
  await gui.pressShortcut("KeyW");
  await waitForPanes(gui, []);
  await gui.request({ type: "close_terminal", terminal: opened.terminal });
  await gui.waitForNone(".sidebar .row:has(.terminal-icon)");
  await gui.pressShortcut("KeyT", { shift: true });
  await waitForPanes(gui, []);
});

e2eTest("removing a session does not add its tab to reopen history", async (environment) => {
  const session = await environment.newSession();
  const gui = await environment.openGui();
  await gui.click(".sidebar .row", "New session");
  await waitForPanes(gui, ["*[*New session]"]);
  await gui.request({ type: "delete_session", session });
  await waitForPanes(gui, []);
  await gui.pressShortcut("KeyT", { shift: true });
  await waitForPanes(gui, []);
});

e2eTest("moving a tab does not add it to reopen history", async (environment) => {
  const { gui } = await twoPanes(environment);
  await gui.dragTab("tallies", 0, "center");
  await waitForPanes(gui, ["*[sh, *tallies]"]);
  await gui.pressShortcut("KeyT", { shift: true });
  await waitForPanes(gui, ["*[sh, *tallies]"]);
});

e2eTest("every tab shows Close Tab without hovering", async (environment) => {
  await environment.newSession();
  const gui = await environment.openGui();
  await gui.showTerminal();
  await gui.click(".sidebar .row", "New session");
  await waitForPanes(gui, ["*[sh, *New session]"]);
  assert.equal(await gui.displayedCount(".tab .tab-close"), 2);
});

e2eTest("dragging the divider between panes resizes them without starting a text selection", async (environment) => {
  const { gui } = await twoPanes(environment);
  const [left] = await gui.paneWidths();
  assert.ok(await gui.dragDivider(".panes .dv-sash", 150), "the divider press can start a text selection");
  await waitFor("the left pane to widen", async () => (await gui.paneWidths())[0] > left + 100);
});

e2eTest("dragging a tab to a pane's center moves the tab there", async (environment) => {
  const { gui } = await twoPanes(environment);
  await gui.dragTab("tallies", 0, "center");
  await waitForPanes(gui, ["*[sh, *tallies]"]);
});

e2eTest("choosing a session or terminal with a tab activates that tab in its pane", async (environment) => {
  const { gui } = await twoPanes(environment);
  await gui.click(".sidebar .row:has(.terminal-icon)");
  await waitForPanes(gui, ["*[*sh]", "[*tallies]"]);
  await gui.waitForText(".sidebar .row.selected", "sh");
  await gui.click(".sidebar .row", "tallies");
  await waitForPanes(gui, ["[*sh]", "*[*tallies]"]);
  await gui.waitForText(".sidebar .row.selected", "tallies");
});

e2eTest("a session chosen in the sidebar opens in the active pane", async (environment) => {
  const { gui } = await twoPanes(environment);
  await environment.ur("new", "home");
  await gui.click(".sidebar .row:has(.terminal-icon)");
  await gui.click(".sidebar .row", "New session");
  await waitForPanes(gui, ["*[sh, *New session]", "[*tallies]"]);
});

e2eTest("the layout and each pane's active tab survive closing and reopening the GUI", async (environment) => {
  const { gui } = await twoPanes(environment);
  await environment.ur("new", "home");
  await gui.click(".sidebar .row:has(.terminal-icon)");
  await gui.click(".sidebar .row", "New session");
  await waitForPanes(gui, ["*[sh, *New session]", "[*tallies]"]);
  await gui.close();

  const reopened = await environment.openGui();
  await waitForPanes(reopened, ["*[sh, *New session]", "[*tallies]"]);
  assert.ok(await reopened.activeTabsVisible(), "a restored active tab is clipped");
});

e2eTest("dragging the sidebar's border resizes it, and the width survives reopening the GUI", async (environment) => {
  const gui = await environment.openGui();
  const width = await gui.sidebarWidth();
  assert.ok(await gui.dragDivider(".sidebar-handle", 100), "the handle press can start a text selection");
  await waitFor("the sidebar to widen", async () => (await gui.sidebarWidth()) === width + 100);
  await gui.close();

  const reopened = await environment.openGui();
  await waitFor("the sidebar to keep its width", async () => (await reopened.sidebarWidth()) === width + 100);
});

e2eTest("Close Tab leaves its terminal running", async (environment) => {
  const gui = await environment.openGui();
  await gui.showTerminal();
  await gui.type("echo before\r");
  await gui.waitForLine("before");
  await gui.click(".tab .tab-close");
  await gui.waitForNone(".tab");
  await gui.waitForText(".sidebar .row:has(.terminal-icon)", "sh");
  await gui.click(".sidebar .row:has(.terminal-icon)");
  await gui.waitForLine("before");
  await gui.type("echo after\r");
  await gui.waitForLine("after");
});

e2eTest("a terminal tab keeps its size while another tab is shown", async (environment) => {
  const session = await environment.newSession();
  await environment.ur("prompt", session, "title");
  const gui = await environment.openGui();
  await gui.showTerminal();
  await gui.type("stty size\r");
  await gui.waitForLine(/^\d+ \d+$/);
  const size = (await gui.lines()).find((line) => /^\d+ \d+$/.test(line))!;
  await gui.click(".sidebar .row", "tallies");
  await waitForPanes(gui, ["*[sh, *tallies]"]);
  await gui.click(".sidebar .row:has(.terminal-icon)");
  await gui.type("clear; stty size\r");
  await waitFor("the size again", async () =>
    (await gui.lines()).some((line) => /^\d+ \d+$/.test(line)),
  );
  assert.equal((await gui.lines()).find((line) => /^\d+ \d+$/.test(line)), size);
});

e2eTest("a session's tab shows its status mark", async (environment) => {
  const session = await environment.newSession();
  const gui = await environment.openGui();
  await gui.click(".sidebar .row", "New session");
  await environment.ur("prompt", session, "tool");
  await gui.waitForText(".tab:has(.status.dot) .label", "New session");
});

e2eTest("a session shown in a pane that is not active does not become unread", async (environment) => {
  const { gui, session } = await twoPanes(environment);
  // The left pane is active, and the right one still shows `tallies`.
  await gui.click(".sidebar .row:has(.terminal-icon)");
  await waitForPanes(gui, ["*[*sh]", "[*tallies]"]);
  await gui.focusWindow();
  await environment.ur("prompt", session, "hello");
  await environment.ur("wait", session);
  // The daemon decides unread when the turn ends, before `ur wait` returns.
  assert.ok(!(await environment.ur("ls")).includes("unread"), "the session became unread");
});

e2eTest("the permission shortcuts answer only the active tab's session", async (environment) => {
  const { gui, session } = await twoPanes(environment);
  const other = await environment.ur("new", "home");
  // The left pane shows `other`, and the right one, which is active, `tallies`.
  await gui.click(".sidebar .row:has(.terminal-icon)");
  await gui.click(".sidebar .row", "New session");
  await gui.click(".sidebar .row", "tallies");
  await waitForPanes(gui, ["[sh, *New session]", "*[*tallies]"]);
  await environment.ur("prompt", other, "tool");
  await environment.ur("prompt", session, "tool");
  await gui.waitForText(".tab:has(.status.dot) .label", "New session");
  await gui.waitForText(".tab:has(.status.dot) .label", "tallies");
  await gui.pressShortcut("KeyY");
  await waitFor("tallies answered", async () =>
    !(await gui.texts(".tab:has(.status.dot) .label")).includes("tallies"),
  );
  assert.deepEqual(await gui.texts(".tab:has(.status.dot) .label"), ["New session"]);
});
