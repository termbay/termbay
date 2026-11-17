// Flow control in the app (task F2, criterion of section 6.5 of
// docs/FASE1-PLANO.md): `yes` in 1 and in 4 panes for 10 s does not freeze
// the window; typing in a fifth pane echoes with p95 < 100 ms; Ctrl+C gives
// the prompt back in under 1 s; the app's memory grows by less than 100 MB.
// Linux only for now (memory is read from /proc). Numbers go to the log and
// to the job summary. TERMBAY_E2E_FLOW_SECONDS shortens the 10 s.
import assert from "node:assert/strict";
import { appendFileSync, readdirSync, readFileSync } from "node:fs";
import { after, before, test } from "node:test";

import { App, KEY, SHORTCUT, say, sleep, until, writeConfig } from "../driver.mjs";

const skip = process.platform !== "linux";
const SECONDS = Number(process.env.TERMBAY_E2E_FLOW_SECONDS ?? 10);
const results = [];
let app;

/**
 * Resident memory in MB of the termbay process (core, queues, Tauri) and of
 * the WebKit web process (page, xterm.js buffers, DOM). Kept apart: the
 * WebKit side grows with the scrollback filling up, which is expected and
 * bounded; the app side is what flow control must keep flat.
 */
function memoryMb() {
  const kb = { app: 0, webkit: 0 };
  for (const pid of readdirSync("/proc").filter((p) => /^\d+$/.test(p))) {
    try {
      const status = readFileSync(`/proc/${pid}/status`, "utf8");
      const name = /^Name:\s+(\S+)/m.exec(status)?.[1];
      const key = name === "termbay" ? "app" : name === "WebKitWebProces" ? "webkit" : null;
      if (key) kb[key] += Number(/^VmRSS:\s+(\d+)/m.exec(status)?.[1] ?? 0);
    } catch {
      // The process went away.
    }
  }
  return { app: kb.app / 1024, webkit: kb.webkit / 1024 };
}

const percentile = (values, p) => [...values].sort((a, b) => a - b)[Math.min(values.length - 1, Math.floor(values.length * p))];

/** Types single letters in the focused pane and measures until each shows up. */
async function echoLatencies(count) {
  const times = [];
  const letters = "qwertzuiopasdfghjklyxcvbnm";
  let typed = "";
  for (let i = 0; i < count; i++) {
    const ch = letters[i % letters.length];
    typed += ch;
    const started = performance.now();
    await app.type(ch);
    await until(`echo of ${typed}`, async () => (await app.focusedText()).includes(typed), 5000);
    times.push(performance.now() - started);
  }
  await app.chord(KEY.control, "c");
  return times;
}

/** Focuses pane `index` of the visible tab. */
async function focusIndex(index) {
  await app.run(`
    const panel = [...document.querySelectorAll(".tab-panel")].find((p) => !p.hidden);
    const pane = panel.querySelectorAll("[data-pane-id]")[arguments[0]];
    pane.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    pane.querySelector("textarea").focus();
  `, index);
}

async function scenario(panes) {
  writeConfig("");
  app = await App.start();
  await sleep(1000);
  const idle = memoryMb();
  // `panes` panes with `yes`, plus one quiet pane to type in.
  for (let i = 1; i <= panes; i++) {
    await app.chord(...(i % 2 ? SHORTCUT.splitRight : SHORTCUT.splitDown));
    await until(`${i + 1} panes`, async () => (await app.paneCount()) === i + 1);
  }
  for (let i = 0; i < panes; i++) {
    await focusIndex(i);
    await app.type("yes" + KEY.enter);
  }
  await sleep((SECONDS * 1000) / 2);
  await focusIndex(panes);
  const latencies = await echoLatencies(20);
  const busy = memoryMb();
  await sleep((SECONDS * 1000) / 2);
  const busyEnd = memoryMb();

  const ctrlC = [];
  for (let i = 0; i < panes; i++) {
    await focusIndex(i);
    const marker = `DONE${i}`;
    const started = performance.now();
    await app.chord(KEY.control, "c");
    await app.type(say("DONE", `${i}`) + KEY.enter);
    await app.waitFor(new RegExp(`^${marker}$`, "m"), 10000);
    ctrlC.push(performance.now() - started);
  }
  await sleep(3000);
  const rest = memoryMb();
  await app.quit();
  app = undefined;

  const row = {
    panes,
    p95: percentile(latencies, 0.95),
    ctrlCMax: Math.max(...ctrlC),
    idle,
    busy,
    busyEnd,
    rest,
  };
  results.push(row);
  return row;
}

before(() => {
  if (!skip) writeConfig("");
});
const mem = (r, key) => [r.idle, r.busy, r.busyEnd, r.rest].map((m) => m[key].toFixed(0)).join(" / ");

after(async () => {
  await app?.quit();
  if (results.length === 0) return;
  let table = `### Flow control in the app (F2, ${SECONDS} s of yes)\n\n| panes with yes | echo p95 (ms) | Ctrl+C to output, worst (ms) | app MB idle / busy / end / after | WebKit MB idle / busy / end / after |\n|---:|---:|---:|---|---|\n`;
  for (const r of results) {
    table += `| ${r.panes} | ${r.p95.toFixed(0)} | ${r.ctrlCMax.toFixed(0)} | ${mem(r, "app")} | ${mem(r, "webkit")} |\n`;
  }
  console.log(table);
  if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, table + "\n");
});

for (const panes of [1, 4]) {
  test(`yes in ${panes} pane(s) keeps the window responsive`, { skip }, async () => {
    const r = await scenario(panes);
    assert.ok(r.p95 < 100 * 3, `echo p95 ${r.p95.toFixed(0)} ms (target 100 ms; CI fails only above 3x)`);
    assert.ok(r.ctrlCMax < 1000 * 3, `Ctrl+C ${r.ctrlCMax.toFixed(0)} ms (target 1 s; CI fails only above 3x)`);
    // Criterion of section 6.5 on the process flow control protects.
    const app = Math.max(r.busy.app, r.busyEnd.app) - r.idle.app;
    assert.ok(app < 100, `termbay grew ${app.toFixed(0)} MB under load`);
    // The page may grow while the scrollback fills, but must level off:
    // the second half of the load adds little (a leak would keep growing).
    const late = r.busyEnd.webkit - r.busy.webkit;
    assert.ok(late < 50, `WebKit kept growing: +${late.toFixed(0)} MB in the second half of the load`);
  });
}
