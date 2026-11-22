// Minimal W3C WebDriver client for the end-to-end tests, talking to
// tauri-driver (https://v2.tauri.app/develop/tests/webdriver/). It uses only
// Node's fetch, so the e2e adds no npm dependency: the handful of commands
// below is all the specs need.
//
// Environment:
//   TERMBAY_E2E_APP     path to the termbay binary built with `npm run build:e2e`
//   TERMBAY_E2E_DRIVER  tauri-driver URL (default http://127.0.0.1:4444)
//   TERMBAY_E2E_CONFIG  config file the app reads (the same path must be in
//                       TERMBAY_CONFIG when tauri-driver starts)

import { writeFileSync } from "node:fs";

const DRIVER = process.env.TERMBAY_E2E_DRIVER ?? "http://127.0.0.1:4444";
const ELEMENT = "element-6066-11e4-a52e-4f735466cecf";

export const KEY = {
  enter: "",
  tab: "",
  escape: "",
  shift: "",
  control: "",
};

export const isWindows = process.platform === "win32";

async function call(method, path, body) {
  const response = await fetch(`${DRIVER}${path}`, {
    method,
    headers: { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const json = await response.json().catch(() => ({}));
  if (!response.ok) throw new Error(`${method} ${path}: ${response.status} ${JSON.stringify(json.value ?? json)}`);
  return json.value;
}

/** Writes the termbay config read by the next app started. */
export function writeConfig(text) {
  const path = process.env.TERMBAY_E2E_CONFIG;
  if (!path) throw new Error("TERMBAY_E2E_CONFIG is not set");
  writeFileSync(path, text);
}

export async function sleep(ms) {
  await new Promise((resolve) => setTimeout(resolve, ms));
}

/** Polls `check` until it returns a truthy value; fails with `what` after `timeout` ms. */
export async function until(what, check, timeout = 15000) {
  const deadline = Date.now() + timeout;
  let last;
  while (Date.now() < deadline) {
    last = await check();
    if (last) return last;
    await sleep(100);
  }
  throw new Error(`timed out waiting for ${what} (last value: ${JSON.stringify(last)})`);
}

export class App {
  constructor(id) {
    this.id = id;
  }

  /** Starts the app and waits for the first pane to render. */
  static async start() {
    const application = process.env.TERMBAY_E2E_APP;
    if (!application) throw new Error("TERMBAY_E2E_APP is not set");
    const session = await call("POST", "/session", {
      capabilities: { alwaysMatch: { "tauri:options": { application } } },
    });
    const app = new App(session.sessionId);
    await until("the first pane", () => app.paneCount());
    await app.focusPane();
    return app;
  }

  async quit() {
    await call("DELETE", `/session/${this.id}`).catch(() => {});
  }

  /** Runs a function in the page; `args` are passed as arguments. */
  async run(script, ...args) {
    return call("POST", `/session/${this.id}/execute/sync`, { script, args });
  }

  /** Sends text and special keys (KEY.*) to the focused element. */
  async type(text) {
    const actions = [];
    for (const ch of text) actions.push({ type: "keyDown", value: ch }, { type: "keyUp", value: ch });
    await this.actions(actions);
  }

  /** Presses a chord, e.g. chord(KEY.control, KEY.shift, "t"). */
  async chord(...keys) {
    const actions = keys.map((value) => ({ type: "keyDown", value }));
    for (const value of [...keys].reverse()) actions.push({ type: "keyUp", value });
    await this.actions(actions);
  }

  async actions(keyActions) {
    await call("POST", `/session/${this.id}/actions`, {
      actions: [{ type: "key", id: "keyboard", actions: keyActions }],
    });
    await call("DELETE", `/session/${this.id}/actions`).catch(() => {});
  }

  /** Clicks the first element matching `selector` (a real WebDriver click). */
  async click(selector) {
    const found = await call("POST", `/session/${this.id}/element`, { using: "css selector", value: selector });
    await call("POST", `/session/${this.id}/element/${found[ELEMENT]}/click`, {});
  }

  /** Clicks the focused pane so its terminal has the keyboard. */
  async focusPane() {
    await this.click(".tab-panel:not([hidden]) [data-pane-id].focused .xterm-screen");
  }

  /** Text of each pane in the visible tab, in DOM order. */
  async paneTexts() {
    return this.run(`
      const panel = [...document.querySelectorAll(".tab-panel")].find((p) => !p.hidden);
      if (!panel) return [];
      return [...panel.querySelectorAll("[data-pane-id]")].map((pane) =>
        [...pane.querySelectorAll(".xterm-rows > div")].map((row) => row.textContent.replace(/\\u00a0/g, " ").trimEnd()).join("\\n"));
    `);
  }

  async paneCount() {
    return (await this.paneTexts()).length;
  }

  /** Text of the focused pane of the visible tab. */
  async focusedText() {
    return this.run(`
      const panel = [...document.querySelectorAll(".tab-panel")].find((p) => !p.hidden);
      const pane = panel?.querySelector("[data-pane-id].focused");
      if (!pane) return "";
      return [...pane.querySelectorAll(".xterm-rows > div")].map((row) => row.textContent.replace(/\\u00a0/g, " ").trimEnd()).join("\\n");
    `);
  }

  /** Waits until the focused pane shows text matching `pattern`. */
  async waitFor(pattern, timeout) {
    try {
      return await until(`${pattern} in the focused pane`, async () => {
        const text = await this.focusedText();
        return pattern.test(text) ? text : false;
      }, timeout);
    } catch (error) {
      // Say where the keyboard and the text went, so a CI failure on a
      // platform we cannot run locally is enough to find the cause.
      const report = await this.focusReport().catch((e) => `focus report failed: ${e.message}`);
      error.message += `\n${report}`;
      throw error;
    }
  }

  /** Which pane has the focus class, which element has the keyboard, and what each pane shows. */
  async focusReport() {
    const state = await this.run(`
      const panel = [...document.querySelectorAll(".tab-panel")].find((p) => !p.hidden);
      const describe = (el) => el ? el.tagName.toLowerCase() + (el.id ? "#" + el.id : "") + (el.className && typeof el.className === "string" ? "." + el.className.trim().split(/\\s+/).join(".") : "") : null;
      const active = document.activeElement;
      return {
        hasFocus: document.hasFocus(),
        activeElement: describe(active),
        activePane: active?.closest?.("[data-pane-id]")?.getAttribute("data-pane-id") ?? null,
        focusedPane: panel?.querySelector("[data-pane-id].focused")?.getAttribute("data-pane-id") ?? null,
        panes: panel ? [...panel.querySelectorAll("[data-pane-id]")].map((pane) => ({
          id: pane.getAttribute("data-pane-id"),
          focused: pane.classList.contains("focused"),
          text: [...pane.querySelectorAll(".xterm-rows > div")].map((row) => row.textContent.replace(/\\u00a0/g, " ").trimEnd()).filter(Boolean).join("\\n"),
        })) : [],
      };
    `);
    return `focus report: ${JSON.stringify(state, null, 2)}`;
  }

  /** Bounding boxes of the panes in the visible tab and of the panel. */
  async geometry() {
    return this.run(`
      const panel = [...document.querySelectorAll(".tab-panel")].find((p) => !p.hidden);
      const box = (el) => { const r = el.getBoundingClientRect(); return { left: r.left, top: r.top, width: r.width, height: r.height }; };
      return { panel: box(panel), panes: [...panel.querySelectorAll("[data-pane-id]")].map(box) };
    `);
  }

  async isVisible(selector) {
    return this.run(`const el = document.querySelector(arguments[0]); return !!el && el.getClientRects().length > 0;`, selector);
  }

  /** Types a command and Enter; with `output`, waits until the focused pane shows it. */
  async runCommand(command, output) {
    await this.type(command + KEY.enter);
    if (output) await this.waitFor(output);
  }
}

/** App shortcuts on Linux and Windows (section 6.6 of docs/FASE1-PLANO.md). */
export const SHORTCUT = {
  newTab: [KEY.control, KEY.shift, "t"],
  closePane: [KEY.control, KEY.shift, "w"],
  splitRight: [KEY.control, KEY.shift, "d"],
  splitDown: [KEY.control, KEY.shift, "s"],
};

/**
 * A command that prints `left + right` without that text appearing in the
 * echo of what was typed, so a match always means the command ran.
 */
export function say(left, right) {
  return isWindows ? `Write-Output ('${left}'+'${right}')` : `echo ${left}""${right}`;
}

/** A foreground program that runs for 30 s (busy pane). */
export const LONG_COMMAND = isWindows ? "ping -n 30 127.0.0.1" : "sleep 30";
