import { type ChildProcess, execFile, spawn } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { connect } from "node:net";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { test } from "node:test";
import { setTimeout as sleep } from "node:timers/promises";
import { promisify } from "node:util";
import { remote } from "webdriverio";

const BIN = resolve(import.meta.dirname, "../../target/e2e/debug");
const ARTIFACTS = resolve(import.meta.dirname, "artifacts");
const WEBDRIVER_PORT = 4445;
const WAIT_MS = 10_000;

type Browser = Awaited<ReturnType<typeof remote>>;

type TauriWindow = {
  __TAURI_INTERNALS__: { invoke(command: string, args: unknown): Promise<unknown> };
};

/** The window with `shown()`, which `Gui.connect()` installs. */
type ShownWindow = {
  shown(selector: string): HTMLElement[];
  pendingFileReads?: Array<(() => Promise<void>) | undefined>;
};

/**
 * One test's temporary directory and daemon, and the GUI launches made in it.
 * The daemon's config file launches the fake server, which keeps its saved
 * history in the directory, so sessions outlive a daemon restart. Its one
 * workspace, `home`, is the test's `HOME`.
 */
export class TestEnvironment {
  readonly home: string;
  readonly #dir: string;
  readonly #socket: string;
  readonly #state: string;
  readonly #config: string;
  #daemon!: ChildProcess;
  #daemonExited!: Promise<void>;
  readonly #guis: Gui[] = [];

  private constructor(dir: string) {
    this.#dir = dir;
    this.#socket = join(dir, "ur.sock");
    this.home = join(dir, "home");
    this.#state = join(dir, "state");
    this.#config = join(dir, "config");
    mkdirSync(this.home);
    mkdirSync(this.#state);
    mkdirSync(join(this.#config, "ur"), { recursive: true });
    const server = JSON.stringify(join(BIN, "ur-fake-server"));
    const history = JSON.stringify(join(dir, "history.json"));
    writeFileSync(
      join(this.#config, "ur/config.toml"),
      `[server]\ncommand = ${server}\nargs = [${history}]\n`,
    );
    this.#spawnDaemon();
  }

  static async start(): Promise<TestEnvironment> {
    const environment = new TestEnvironment(mkdtempSync(join(tmpdir(), "ur-e2e-")));
    // The core retries until the daemon is up, but waiting here keeps the
    // no-connection state out of the tests.
    await waitFor("the daemon to listen", () => canConnect(environment.#socket));
    await environment.ur("workspace", "add", "home", environment.home);
    return environment;
  }

  #spawnDaemon(): void {
    // `/bin/sh` keeps user shell configuration out of the terminals.
    this.#daemon = spawn(join(BIN, "ur"), ["daemon"], {
      env: {
        ...process.env,
        UR_SOCKET: this.#socket,
        HOME: this.home,
        XDG_CONFIG_HOME: this.#config,
        XDG_STATE_HOME: this.#state,
        SHELL: "/bin/sh",
      },
      stdio: ["ignore", "inherit", "inherit"],
    });
    // Listen now: the daemon can exit before `stop`, such as when it crashes.
    this.#daemonExited = new Promise((resolve) => this.#daemon.once("exit", () => resolve()));
  }

  /** Kills the daemon and waits for it to exit. */
  async stopDaemon(): Promise<void> {
    this.#daemon.kill("SIGKILL");
    await this.#daemonExited;
  }

  /** Starts the daemon again after `stopDaemon()` and waits for it to listen. */
  async startDaemon(): Promise<void> {
    this.#spawnDaemon();
    await waitFor("the daemon to listen", () => canConnect(this.#socket));
  }

  /** Runs the `ur` command line against the daemon and returns its output. */
  async ur(...args: string[]): Promise<string> {
    const { stdout } = await promisify(execFile)(join(BIN, "ur"), args, {
      env: { ...process.env, UR_SOCKET: this.#socket },
    });
    return stdout.trim();
  }

  /** Creates a session in the `home` workspace. */
  async newSession(): Promise<string> {
    return this.ur("new", "home");
  }

  /** Starts `ur-app`, connects to its WebDriver server, and waits for the sidebar. */
  async openGui(): Promise<Gui> {
    const child = spawn(join(BIN, "ur-app"), [], {
      env: {
        ...process.env,
        UR_SOCKET: this.#socket,
        XDG_STATE_HOME: this.#state,
        TAURI_WEBDRIVER_PORT: String(WEBDRIVER_PORT),
      },
      stdio: ["ignore", "inherit", "inherit"],
    });
    const exited = new Promise<void>((resolve) => child.once("exit", () => resolve()));
    const gui = new Gui(child, exited);
    this.#guis.push(gui);
    await waitFor("the WebDriver server", webdriverReady);
    await gui.connect();
    await gui.focusWindow();
    // The workspace comes with the watch snapshot.
    await waitFor("the sidebar to render", () => gui.hasElement(".workspace"));
    return gui;
  }

  /** Saves a screenshot of the most recent GUI still open, if any. */
  async screenshot(name: string): Promise<void> {
    const gui = this.#guis.findLast((gui) => gui.isOpen);
    if (gui !== undefined) {
      mkdirSync(ARTIFACTS, { recursive: true });
      await gui.screenshot(join(ARTIFACTS, `${name.replaceAll(/\W+/g, "-")}.png`));
    }
  }

  async stop(): Promise<void> {
    for (const gui of this.#guis) {
      await gui.close();
    }
    await this.stopDaemon();
    // The shells and their applications get SIGHUP when the daemon exits and
    // can still be writing files such as `.viminfo`, so retry the removal.
    await waitFor("the test directory to be removed", async () => {
      try {
        rmSync(this.#dir, { recursive: true, force: true });
        return true;
      } catch {
        return false;
      }
    });
  }
}

/** One running `ur-app` process and its WebDriver session. */
export class Gui {
  readonly #process: ChildProcess;
  readonly #exited: Promise<void>;
  #browser: Browser | undefined;

  constructor(process: ChildProcess, exited: Promise<void>) {
    this.#process = process;
    this.#exited = exited;
  }

  get isOpen(): boolean {
    return this.#process.exitCode === null && this.#process.signalCode === null;
  }

  async connect(): Promise<void> {
    this.#browser = await remote({
      hostname: "127.0.0.1",
      port: WEBDRIVER_PORT,
      capabilities: {},
      logLevel: "warn",
    });
    // The server is ready once the window exists, which can be before the
    // webview leaves `about:blank`. A script run then loses its result with the
    // page and waits out the script timeout.
    await waitFor("the webview to load the app", async () => {
      return (await this.#session().getUrl()) !== "about:blank";
    });
    // Every helper looks only at shown elements. A tab that is not active
    // stays mounted, hidden with `visibility: hidden`.
    await this.#session().execute(() => {
      (window as unknown as ShownWindow).shown = (selector) =>
        Array.from(document.querySelectorAll<HTMLElement>(selector)).filter(
          (element) => getComputedStyle(element).visibility !== "hidden",
        );
    });
  }

  /**
   * Makes `ur-app` the frontmost application. The GUI focuses its visible
   * sessions only while its window has focus, and WebDriver cannot focus the
   * window.
   */
  async focusWindow(): Promise<void> {
    const script = `tell application "System Events" to set frontmost of (first process whose unix id is ${this.#process.pid}) to true`;
    await promisify(execFile)("osascript", ["-e", script]);
    await waitFor("the window to have focus", () =>
      this.#session().execute(() => document.hasFocus()),
    );
  }

  /**
   * Types `text` as xterm.js receives real typing: an `insertText` input event
   * reaches `onData` unchanged. WebDriver key actions send key codes xterm.js
   * misreads, and each character twice.
   */
  async type(text: string): Promise<void> {
    await this.#session().execute((text) => {
      const textarea = (window as unknown as ShownWindow).shown(".xterm-helper-textarea")[0];
      textarea.dispatchEvent(new InputEvent("input", { data: text, inputType: "insertText" }));
    }, text);
  }

  async hasElement(selector: string): Promise<boolean> {
    return this.#session().execute(
      (selector) => (window as unknown as ShownWindow).shown(selector).length > 0,
      selector,
    );
  }

  /**
   * Whether the editor's bottom row fits its controls on one line. The row
   * keeps hidden copies of its config pickers to measure them.
   */
  async editorControlsFit(): Promise<boolean> {
    return this.#session().execute(() => {
      const actions = (window as unknown as ShownWindow).shown(".editor-actions")[0];
      const controls = (window as unknown as ShownWindow)
        .shown(".editor-actions .picker, .editor-actions .more-options, .editor-actions .usage, .editor-actions > button")
        .filter((element) => element.getClientRects().length > 0);
      const centers = controls.map((element) => {
        const box = element.getBoundingClientRect();
        return box.top + box.height / 2;
      });
      return actions.scrollWidth <= actions.clientWidth + 1 &&
        Math.max(...centers) - Math.min(...centers) < 2;
    });
  }

  /** Whether every shown element matching `selector` lies inside the window. */
  async insideWindow(selector: string): Promise<boolean> {
    return this.#session().execute(
      (selector) =>
        (window as unknown as ShownWindow).shown(selector).every((element) => {
          const box = element.getBoundingClientRect();
          return box.left >= 0 && box.top >= 0 && box.right <= window.innerWidth &&
            box.bottom <= window.innerHeight;
        }),
      selector,
    );
  }

  /** How many shown elements match `selector` and take up space on the page. */
  async displayedCount(selector: string): Promise<number> {
    return this.#session().execute(
      (selector) =>
        (window as unknown as ShownWindow)
          .shown(selector)
          .filter((element) => element.getClientRects().length > 0).length,
      selector,
    );
  }

  /** Whether every active tab is fully visible in its pane header. */
  async activeTabsVisible(): Promise<boolean> {
    return this.#session().execute(() => {
      const tabs = document.querySelectorAll<HTMLElement>(
        ".panes .dv-tabs-container .dv-tab.dv-active-tab",
      );
      return Array.from(tabs).every((tab) => {
        const container = tab.closest<HTMLElement>(".dv-tabs-container")!;
        const bounds = tab.getBoundingClientRect();
        const viewport = container.getBoundingClientRect();
        return bounds.left >= viewport.left - 1 && bounds.right <= viewport.right + 1;
      });
    });
  }

  /**
   * Opens a terminal in the `home` workspace and opens its tab by clicking its
   * terminal row, unless a reopened GUI restored the tab from the layout, and
   * waits for the terminal to render.
   */
  async showTerminal(): Promise<void> {
    if (!(await this.hasElement(".xterm"))) {
      // WebDriver cannot drive the native workspace menu.
      await this.request({ type: "open_terminal", workspace: "home" });
      await this.click(".sidebar .row:has(.terminal-icon)");
    }
    await waitFor("the terminal to render", async () =>
      (await this.lines()).some((row) => row !== ""),
    );
  }

  /** Clicks the first element matching `selector` whose text contains `text`. */
  async click(selector: string, text = ""): Promise<void> {
    await waitFor(`${selector} with ${JSON.stringify(text)}`, () =>
      this.#session().execute(
        (selector, text) => {
          const element = Array.from((window as unknown as ShownWindow).shown(selector)).find(
            (element) => element.textContent!.includes(text),
          );
          element?.click();
          return element !== undefined;
        },
        selector,
        text,
      ),
    );
  }

  /**
   * Presses the first element matching `selector` whose text contains `text`,
   * as a mouse does: mousedown, mouseup, then click. `click()` alone sends no
   * press, so it cannot exercise code that listens for one.
   */
  async press(selector: string, text = ""): Promise<void> {
    await this.#session().execute(
      (selector, text) => {
        const element = Array.from((window as unknown as ShownWindow).shown(selector)).find(
          (element) => element.textContent!.includes(text),
        )!;
        const init = { bubbles: true, cancelable: true };
        element.dispatchEvent(new MouseEvent("mousedown", init));
        element.dispatchEvent(new MouseEvent("mouseup", init));
        element.click();
      },
      selector,
      text,
    );
  }

  /**
   * Whether the webview canceled a `mousedown` on the first element matching
   * `selector`. Script-dispatched events never start a text selection, so the
   * canceled press stands in for the one a real press would start.
   */
  async mouseDownCanceled(selector: string): Promise<boolean> {
    return this.#session().execute(
      (selector) =>
        !(window as unknown as ShownWindow)
          .shown(selector)[0]
          .dispatchEvent(new MouseEvent("mousedown", { bubbles: true, cancelable: true, button: 0 })),
      selector,
    );
  }

  /** The text of each element matching `selector`. */
  async texts(selector: string): Promise<string[]> {
    return this.#session().execute(
      (selector) => (window as unknown as ShownWindow).shown(selector).map((element) => element.textContent!),
      selector,
    );
  }

  /** The values of `attribute` on shown elements matching `selector`. */
  async attributes(selector: string, attribute: string): Promise<(string | null)[]> {
    return this.#session().execute(
      (selector, attribute) =>
        (window as unknown as ShownWindow).shown(selector).map((element) => element.getAttribute(attribute)),
      selector,
      attribute,
    );
  }

  /** Waits until an element matching `selector` has text containing `text`. */
  async waitForText(selector: string, text: string): Promise<void> {
    let texts: string[] = [];
    try {
      await waitFor(`${selector} with ${JSON.stringify(text)}`, async () => {
        texts = await this.texts(selector);
        return texts.some((element) => element.includes(text));
      });
    } catch (error) {
      throw new Error(`no ${selector} with ${JSON.stringify(text)} in:\n${texts.join("\n")}`, {
        cause: error,
      });
    }
  }

  /** Waits until no element matches `selector`. */
  async waitForNone(selector: string): Promise<void> {
    let texts: string[] = [];
    try {
      await waitFor(`no ${selector}`, async () => {
        texts = await this.texts(selector);
        return texts.length === 0;
      });
    } catch (error) {
      throw new Error(`still ${selector}:\n${texts.join("\n")}`, { cause: error });
    }
  }

  /** Types `text` into the editor and presses Enter. */
  async sendPrompt(text: string): Promise<void> {
    await this.typePrompt(text);
    await this.pressKey(".editor textarea", "Enter");
  }

  /** Replaces the editor's text with `text`, as typing it would. */
  async typePrompt(text: string): Promise<void> {
    await this.#session().execute((text) => {
      const shown = (window as unknown as ShownWindow).shown;
      const textarea = shown(".editor textarea")[0] as HTMLTextAreaElement;
      // React tracks the value it set, so set it the way typing does.
      const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!;
      setter.call(textarea, text);
      textarea.dispatchEvent(new Event("input", { bubbles: true }));
    }, text);
  }

  /** Presses `key` in the first element matching `selector`, as a keydown event. */
  async pressKey(selector: string, key: string): Promise<void> {
    await this.#session().execute(
      (selector, key) => {
        const event = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true });
        (window as unknown as ShownWindow).shown(selector)[0].dispatchEvent(event);
      },
      selector,
      key,
    );
  }

  /** The editor's text. */
  async promptText(): Promise<string> {
    return this.#session().execute(
      () =>
        ((window as unknown as ShownWindow).shown(".editor textarea")[0] as HTMLTextAreaElement)
          .value,
    );
  }

  /** How many lines of text the editor's textarea is high. */
  async promptLines(): Promise<number> {
    return this.#session().execute(() => {
      const textarea = (window as unknown as ShownWindow).shown(".editor textarea")[0];
      return Math.round(textarea.clientHeight / parseFloat(getComputedStyle(textarea).lineHeight));
    });
  }

  /**
   * Presses ⌘ with the key whose `KeyboardEvent.code` is `code`, and ⇧ or ⌥
   * as given, on the window, where the permission shortcuts listen.
   */
  async pressShortcut(code: string, { shift = false, alt = false } = {}): Promise<void> {
    await this.#session().execute(
      (code, shift, alt) => {
        const event = new KeyboardEvent("keydown", {
          code,
          metaKey: true,
          shiftKey: shift,
          altKey: alt,
          bubbles: true,
          cancelable: true,
        });
        window.dispatchEvent(event);
      },
      code,
      shift,
      alt,
    );
  }

  /**
   * Clicks the element matching `selector` and returns the text it writes to
   * the clipboard, which is recorded instead of written.
   */
  async clickCopy(selector: string): Promise<string> {
    return this.#session().execute(
      (selector) =>
        new Promise<string>((resolve) => {
          navigator.clipboard.writeText = async (text) => resolve(text);
          (window as unknown as ShownWindow).shown(selector)[0].click();
        }),
      selector,
    );
  }

  /**
   * Moves the pointer onto the first element matching `selector`, as a
   * `mouseover` from outside it, which React reports as `onMouseEnter`.
   */
  async hover(selector: string): Promise<void> {
    await waitFor(`${selector} to hover`, () =>
      this.#session().execute((selector) => {
        const element = (window as unknown as ShownWindow).shown(selector).at(0);
        const init = { bubbles: true, relatedTarget: document.body };
        element?.dispatchEvent(new MouseEvent("mouseover", init));
        return element !== undefined;
      }, selector),
    );
  }

  /** Holds subsequent file reads until `releaseFileRead` starts each one. */
  async holdFileReads(): Promise<void> {
    await this.#session().execute(() => {
      const pending: Array<(() => Promise<void>) | undefined> = [];
      const read = FileReader.prototype.readAsDataURL;
      FileReader.prototype.readAsDataURL = function (blob) {
        const reader = this;
        pending.push(() => new Promise((resolve) => {
          reader.addEventListener("loadend", () => resolve(), { once: true });
          read.call(reader, blob);
        }));
      };
      (window as unknown as ShownWindow).pendingFileReads = pending;
    });
  }

  /** Starts one held file read by its zero-based drop order. */
  async releaseFileRead(index: number): Promise<void> {
    await this.#session().execute((index) => {
      const pending = (window as unknown as ShownWindow).pendingFileReads!;
      const read = pending[index];
      if (read === undefined) {
        throw new Error(`no held file read at ${index}`);
      }
      pending[index] = undefined;
      return read();
    }, index);
  }

  /** Drops files on the first shown element matching `selector`, in their given order. */
  async dropFiles(
    selector: string,
    files: Array<{ name: string; type: string; data: string }>,
  ): Promise<void> {
    await this.#session().execute(
      (selector, files) => {
        const transfer = new DataTransfer();
        for (const { name, type, data } of files) {
          const bytes = Uint8Array.from(atob(data), (char) => char.charCodeAt(0));
          transfer.items.add(new File([bytes], name, { type }));
        }
        const target = (window as unknown as ShownWindow).shown(selector)[0];
        const init = { bubbles: true, cancelable: true, dataTransfer: transfer };
        target.dispatchEvent(new DragEvent("dragover", init));
        target.dispatchEvent(new DragEvent("drop", init));
      },
      selector,
      files,
    );
  }

  /** Drops one base64-encoded file on the first shown element matching `selector`. */
  async dropFile(selector: string, name: string, type: string, data: string): Promise<void> {
    await this.dropFiles(selector, [{ name, type, data }]);
  }

  /**
   * The tab labels of each pane, in order, with `*` before each pane's active
   * tab, and `*` before the active pane's list.
   */
  async panes(): Promise<string[]> {
    return this.#session().execute(() =>
      Array.from(document.querySelectorAll(".dv-groupview"), (pane) => {
        const tabs = Array.from(pane.querySelectorAll(".dv-tab"), (tab) => {
          const label = tab.querySelector(".tab .label")!.textContent!;
          return tab.classList.contains("dv-active-tab") ? `*${label}` : label;
        });
        const active = pane.classList.contains("dv-active-group") ? "*" : "";
        return `${active}[${tabs.join(", ")}]`;
      }),
    );
  }

  /**
   * Drags the tab whose label contains `label` onto pane `pane`, counted from
   * zero: its right edge, which makes a new pane, or its center, which moves
   * the tab there, with the pointer events dockview listens for.
   */
  async dragTab(label: string, pane: number, where: "right" | "center"): Promise<void> {
    await this.#session().execute(
      async (label, pane, where) => {
        const tab = Array.from(document.querySelectorAll(".dv-tab")).find((tab) =>
          tab.querySelector(".tab .label")!.textContent!.includes(label),
        )!;
        const start = tab.getBoundingClientRect();
        const content = document.querySelectorAll(".dv-groupview .dv-content-container")[pane];
        const box = content.getBoundingClientRect();
        const x = where === "right" ? box.right - 5 : box.left + box.width / 2;
        const y = box.top + box.height / 2;
        const init = { bubbles: true, cancelable: true, pointerId: 1, pointerType: "mouse", isPrimary: true };
        const startX = start.left + start.width / 2;
        const startY = start.top + start.height / 2;
        tab.dispatchEvent(
          new PointerEvent("pointerdown", { ...init, clientX: startX, clientY: startY, button: 0, buttons: 1 }),
        );
        for (const [moveX, moveY] of [[startX + 20, startY + 20], [x, y]]) {
          await new Promise((resolve) => setTimeout(resolve, 50));
          window.dispatchEvent(new PointerEvent("pointermove", { ...init, clientX: moveX, clientY: moveY, buttons: 1 }));
        }
        await new Promise((resolve) => setTimeout(resolve, 50));
        window.dispatchEvent(new PointerEvent("pointerup", { ...init, clientX: x, clientY: y, button: 0 }));
      },
      label,
      pane,
      where,
    );
  }

  /** Each pane's width, left to right. */
  async paneWidths(): Promise<number[]> {
    return this.#session().execute(() =>
      Array.from(document.querySelectorAll(".dv-groupview")).map(
        (pane) => pane.getBoundingClientRect().width,
      ),
    );
  }

  /** The sidebar's width. */
  async sidebarWidth(): Promise<number> {
    return this.#session().execute(() => document.querySelector(".sidebar")!.getBoundingClientRect().width);
  }

  /**
   * Drags the first shown divider matching `selector` `dx` pixels, with the
   * pointer events the app and dockview listen for, and answers whether the
   * webview canceled the press. Script-dispatched events never start a text
   * selection, so the canceled press stands in for the one a real press would
   * start.
   */
  async dragDivider(selector: string, dx: number): Promise<boolean> {
    return this.#session().execute(async (selector, dx) => {
      const divider = Array.from(document.querySelectorAll(selector)).find(
        (element) => element.getBoundingClientRect().height > 0,
      )!;
      const box = divider.getBoundingClientRect();
      const x = box.left + box.width / 2;
      const y = box.top + box.height / 2;
      const init = { bubbles: true, cancelable: true, pointerId: 1, isPrimary: true, clientY: y };
      const pressed = divider.dispatchEvent(
        new PointerEvent("pointerdown", { ...init, clientX: x, button: 0, buttons: 1 }),
      );
      for (const step of [dx / 2, dx]) {
        await new Promise((resolve) => setTimeout(resolve, 20));
        document.dispatchEvent(new PointerEvent("pointermove", { ...init, clientX: x + step, buttons: 1 }));
      }
      document.dispatchEvent(new PointerEvent("pointerup", { ...init, clientX: x + dx, button: 0 }));
      return !pressed;
    }, selector, dx);
  }

  /** The webview's URL. */
  async url(): Promise<string> {
    return this.#session().getUrl();
  }

  /** Scrolls the thread to `top` pixels. */
  async scrollThread(top: number): Promise<void> {
    await this.#session().execute((top) => {
      (window as unknown as ShownWindow).shown(".thread")[0].scrollTop = top;
    }, top);
  }

  /** The thread's scroll position in pixels. */
  async threadScrollTop(): Promise<number> {
    return this.#session().execute(() =>
      (window as unknown as ShownWindow).shown(".thread")[0].scrollTop,
    );
  }

  /**
   * Prompts `session` with `text` and one image of `bytes` random bytes,
   * built in the webview so it does not cross WebDriver.
   */
  async promptWithImage(session: string, text: string, bytes: number): Promise<void> {
    await this.#session().execute(
      async (session, text, bytes) => {
        const data = new Uint8Array(bytes);
        // `getRandomValues` fills at most 64 KiB per call.
        for (let offset = 0; offset < bytes; offset += 65536) {
          crypto.getRandomValues(data.subarray(offset, offset + 65536));
        }
        let binary = "";
        for (let offset = 0; offset < bytes; offset += 65536) {
          binary += String.fromCharCode(...data.subarray(offset, offset + 65536));
        }
        const content = [
          { type: "text", text },
          { type: "image", mimeType: "image/png", data: btoa(binary) },
        ];
        const tauri = (window as unknown as TauriWindow).__TAURI_INTERNALS__;
        await tauri.invoke("request", { request: { type: "prompt", session, content } });
      },
      session,
      text,
      bytes,
    );
  }

  /** Sends `request` through the core's `request` command and returns the response. */
  async request(request: object): Promise<unknown> {
    return this.#session().execute((request) => {
      const tauri = (window as unknown as TauriWindow).__TAURI_INTERNALS__;
      return tauri.invoke("request", { request });
    }, request);
  }

  /** The terminal's rows as rendered, with trailing spaces removed. */
  async lines(): Promise<string[]> {
    const rows = await this.#session().execute(() =>
      (window as unknown as ShownWindow)
        .shown(".xterm-rows > div")
        .map((row) => row.textContent ?? ""),
    );
    return rows.map((row) => row.replaceAll("\u00a0", " ").trimEnd());
  }

  /**
   * Waits until a row matches: a string must equal the row without its
   * surrounding spaces, and a pattern must match somewhere in the row.
   */
  async waitForLine(line: string | RegExp): Promise<void> {
    const matches = (row: string) =>
      typeof line === "string" ? row.trim() === line : line.test(row);
    let rows: string[] = [];
    try {
      await waitFor(`a line ${line}`, async () => {
        rows = await this.lines();
        return rows.some(matches);
      });
    } catch (error) {
      throw new Error(`no line ${line} on screen:\n${rows.join("\n")}`, { cause: error });
    }
  }

  async setWindowSize(width: number, height: number): Promise<void> {
    await this.#session().setWindowSize(width, height);
  }

  async screenshot(path: string): Promise<void> {
    await this.#session().saveScreenshot(path);
  }

  /** Kills `ur-app`; the daemon sees its socket connection end. */
  async close(): Promise<void> {
    if (this.isOpen) {
      this.#process.kill("SIGKILL");
    }
    await this.#exited;
  }

  #session(): Browser {
    if (this.#browser === undefined) {
      throw new Error("the GUI has no WebDriver session");
    }
    return this.#browser;
  }
}

/**
 * `node:test`'s `test`, given a started `TestEnvironment`. Saves a screenshot
 * to `e2e/artifacts/` when the test fails.
 */
export function e2eTest(name: string, body: (environment: TestEnvironment) => Promise<void>) {
  test(name, async () => {
    const environment = await TestEnvironment.start();
    try {
      await body(environment);
    } catch (error) {
      // A failed screenshot must not replace the test's error.
      await environment.screenshot(name).catch((screenshotError) => {
        console.error(`no screenshot: ${screenshotError}`);
      });
      throw error;
    } finally {
      await environment.stop();
    }
  });
}

export async function waitFor(what: string, ready: () => Promise<boolean>): Promise<void> {
  const deadline = Date.now() + WAIT_MS;
  while (!(await ready())) {
    if (Date.now() > deadline) {
      throw new Error(`timed out waiting for ${what}`);
    }
    await sleep(50);
  }
}

function canConnect(socket: string): Promise<boolean> {
  return new Promise((resolve) => {
    const connection = connect(socket);
    connection.once("connect", () => {
      connection.destroy();
      resolve(true);
    });
    connection.once("error", () => resolve(false));
  });
}

async function webdriverReady(): Promise<boolean> {
  try {
    const response = await fetch(`http://127.0.0.1:${WEBDRIVER_PORT}/status`);
    const { value } = await response.json();
    return value.ready === true;
  } catch {
    return false;
  }
}
