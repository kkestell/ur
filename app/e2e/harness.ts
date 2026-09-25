import { type ChildProcess, spawn } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { connect } from "node:net";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { test } from "node:test";
import { setTimeout as sleep } from "node:timers/promises";
import { remote } from "webdriverio";

const BIN = resolve(import.meta.dirname, "../../target/e2e/debug");
const ARTIFACTS = resolve(import.meta.dirname, "artifacts");
const WEBDRIVER_PORT = 4445;
const WAIT_MS = 10_000;

type Browser = Awaited<ReturnType<typeof remote>>;

/**
 * One test's temporary directory and daemon, and the GUI launches made in it.
 * The daemon's config file launches the fake server.
 */
export class TestEnvironment {
  readonly home: string;
  readonly #dir: string;
  readonly #socket: string;
  readonly #state: string;
  readonly #daemon: ChildProcess;
  readonly #daemonExited: Promise<void>;
  readonly #guis: Gui[] = [];

  private constructor(dir: string) {
    this.#dir = dir;
    this.#socket = join(dir, "ur.sock");
    this.home = join(dir, "home");
    this.#state = join(dir, "state");
    const config = join(dir, "config");
    mkdirSync(this.home);
    mkdirSync(this.#state);
    mkdirSync(join(config, "ur"), { recursive: true });
    writeFileSync(
      join(config, "ur/config.toml"),
      `[server]\ncommand = ${JSON.stringify(join(BIN, "ur-fake-server"))}\n`,
    );
    // `/bin/sh` keeps user shell configuration out of the terminals.
    this.#daemon = spawn(join(BIN, "ur"), ["daemon"], {
      env: {
        ...process.env,
        UR_SOCKET: this.#socket,
        HOME: this.home,
        XDG_CONFIG_HOME: config,
        XDG_STATE_HOME: this.#state,
        SHELL: "/bin/sh",
      },
      stdio: ["ignore", "inherit", "inherit"],
    });
    // Listen now: the daemon can exit before `stop`, such as when it crashes.
    this.#daemonExited = new Promise((resolve) => this.#daemon.once("exit", () => resolve()));
  }

  static async start(): Promise<TestEnvironment> {
    const environment = new TestEnvironment(mkdtempSync(join(tmpdir(), "ur-e2e-")));
    // The core connects once, so the daemon must be listening first.
    await waitFor("the daemon to listen", () => canConnect(environment.#socket));
    return environment;
  }

  /** Starts `ur-app` and connects to its WebDriver server once the terminal renders. */
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
    await waitFor("the terminal to render", async () =>
      (await gui.lines()).some((row) => row !== ""),
    );
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
    this.#daemon.kill("SIGKILL");
    await this.#daemonExited;
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
  }

  /**
   * Types `text` as xterm.js receives real typing: an `insertText` input event
   * reaches `onData` unchanged. WebDriver key actions send key codes xterm.js
   * misreads, and each character twice.
   */
  async type(text: string): Promise<void> {
    await this.#session().execute((text) => {
      const textarea = document.querySelector(".xterm-helper-textarea")!;
      textarea.dispatchEvent(new InputEvent("input", { data: text, inputType: "insertText" }));
    }, text);
  }

  /** The terminal's rows as rendered, with trailing spaces removed. */
  async lines(): Promise<string[]> {
    const rows = await this.#session().execute(() =>
      Array.from(document.querySelectorAll(".xterm-rows > div"), (row) => row.textContent ?? ""),
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
