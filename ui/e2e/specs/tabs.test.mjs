// Milestone 2 end to end (task F3): tabs, splits, closing and confirmation.
import assert from "node:assert/strict";
import { afterEach, test } from "node:test";

import { App, KEY, LONG_COMMAND, SHORTCUT, isWindows, say, sleep, until, writeConfig } from "../driver.mjs";

let app;
async function start() {
  writeConfig("");
  app = await App.start();
  return app;
}
afterEach(async () => app?.quit());

test("two tabs keep their own output", async () => {
  await start();
  await app.runCommand(say("TAB", "_ONE"), /TAB_ONE/);
  await app.chord(...SHORTCUT.newTab);
  await until("the second tab", async () => (await app.run(`return document.querySelectorAll(".tab").length`)) === 2);
  await app.focusPane();
  await app.runCommand(say("TAB", "_TWO"), /TAB_TWO/);
  assert.doesNotMatch(await app.focusedText(), /TAB_ONE/);

  await app.click(".tab[data-tab-id]");
  await app.waitFor(/TAB_ONE/);
  assert.doesNotMatch(await app.focusedText(), /TAB_TWO/);
});

test("split right and down, and closing a pane gives its space back", async () => {
  await start();
  await app.chord(...SHORTCUT.splitRight);
  await until("two panes", async () => (await app.paneCount()) === 2);
  let g = await app.geometry();
  assert.ok(g.panes[1].left > g.panes[0].left + 10, `right split: ${JSON.stringify(g.panes)}`);
  assert.ok(Math.abs(g.panes[1].top - g.panes[0].top) < 2);

  await app.chord(...SHORTCUT.splitDown);
  await until("three panes", async () => (await app.paneCount()) === 3);
  g = await app.geometry();
  assert.ok(g.panes[2].top > g.panes[1].top + 10, `down split: ${JSON.stringify(g.panes)}`);

  // The new pane gets the focus and a shell of its own.
  await app.runCommand(say("SPLIT", "_PANE"), /SPLIT_PANE/);

  // Idle shells close without asking.
  await app.chord(...SHORTCUT.closePane);
  await until("two panes", async () => (await app.paneCount()) === 2);
  await app.chord(...SHORTCUT.closePane);
  await until("one pane", async () => (await app.paneCount()) === 1);
  g = await app.geometry();
  assert.ok(Math.abs(g.panes[0].width - g.panel.width) < 4, `collapsed: ${JSON.stringify(g)}`);
  assert.ok(Math.abs(g.panes[0].height - g.panel.height) < 4, `collapsed: ${JSON.stringify(g)}`);
});

test("closing a pane with a running program asks first", async () => {
  await start();
  await app.chord(...SHORTCUT.splitRight);
  await until("two panes", async () => (await app.paneCount()) === 2);
  await app.focusPane();
  await app.type(LONG_COMMAND + KEY.enter);
  await sleep(1500); // pty_info has to see the foreground program

  await app.chord(...SHORTCUT.closePane);
  await until("the confirmation", () => app.isVisible(".confirm"));
  await app.click(".confirm-cancel");
  await until("the confirmation to go", async () => !(await app.isVisible(".confirm")));
  assert.equal(await app.paneCount(), 2, "cancel keeps the pane");

  await app.chord(...SHORTCUT.closePane);
  await until("the confirmation", () => app.isVisible(".confirm"));
  await app.click(".confirm-ok");
  await until("one pane", async () => (await app.paneCount()) === 1);
});

test("exit closes the pane and exit 1 keeps it", async () => {
  await start();
  await app.chord(...SHORTCUT.splitRight);
  await until("two panes", async () => (await app.paneCount()) === 2);
  await app.focusPane();
  await app.runCommand(say("READY", "_TO_EXIT"), /READY_TO_EXIT/);
  await app.type("exit" + KEY.enter);
  await until("one pane", async () => (await app.paneCount()) === 1);

  await app.chord(...SHORTCUT.splitRight);
  await until("two panes", async () => (await app.paneCount()) === 2);
  await app.focusPane();
  await app.runCommand(say("READY", "_TO_FAIL"), /READY_TO_FAIL/);
  await app.type("exit 1" + KEY.enter);
  await sleep(2000);
  assert.equal(await app.paneCount(), 2, "a failed exit keeps the pane so the error can be read");
});

test("Ctrl+R reaches the shell (history search, not a page reload)", async () => {
  await start();
  await app.runCommand(say("BEFORE", "_SEARCH"), /BEFORE_SEARCH/);
  await app.chord(KEY.control, "r");
  // bash: (reverse-i-search); PowerShell (PSReadLine): bck-i-search.
  await app.waitFor(/i-search/);
  await app.chord(KEY.control, "c");
  // A reload would have cleared the terminal.
  assert.match(await app.focusedText(), /BEFORE_SEARCH/);
});

test("the WebView2 browser keys do not reload the page", { skip: !isWindows }, async () => {
  await start();
  await app.runCommand(say("BEFORE", "_F5"), /BEFORE_F5/);
  await app.type(""); // F5
  await sleep(1000);
  assert.match(await app.focusedText(), /BEFORE_F5/);
});
