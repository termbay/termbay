// P8 (docs/SEGURANCA.md): the autocomplete in the whole app, Linux only.
// The CI runs this file once per shell, after making it the login shell
// (`chsh`), with TERMBAY_E2E_SHELL=zsh or fish. Without it, it is skipped.
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, before, test } from "node:test";

import { App, KEY, say, sleep, until, writeConfig } from "../driver.mjs";

const shell = process.env.TERMBAY_E2E_SHELL;
const skip = !shell || process.platform !== "linux";
let dir;
let app;

before(() => {
  if (skip) return;
  dir = mkdtempSync(join(tmpdir(), "termbay-p8-"));
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "fixture-one.txt"), "");
  writeFileSync(join(dir, "fixture-two.txt"), "");
});
afterEach(async () => app?.quit());

async function start(enabled) {
  writeConfig(`[autocomplete]\nenabled = ${enabled}\n`);
  app = await App.start();
  await app.runCommand(`cd ${dir}; ${say("IN", "_P8_DIR")}`, /IN_P8_DIR/);
}

const menuOpen = () => app.isVisible(".termbay-autocomplete-option");

test(`${shell}: ls fix + Tab opens the menu, Enter inserts and does not run`, { skip }, async () => {
  await start(true);
  await app.type("ls fix" + KEY.tab);
  await until("the autocomplete menu", menuOpen);
  const options = await app.run(
    `return [...document.querySelectorAll(".termbay-autocomplete-option")].map((o) => o.textContent)`,
  );
  assert.ok(options.some((o) => o.includes("fixture-one.txt")), JSON.stringify(options));

  await app.type(KEY.enter);
  await until("the menu to close", async () => !(await menuOpen()));
  await app.waitFor(/ls fixture-\S+/);
  await sleep(800);
  const lines = (await app.focusedText()).split("\n");
  // `ls` alone on a line would print both names; the command line has one.
  assert.ok(!lines.some((l) => /fixture-one\.txt\s+fixture-two\.txt/.test(l)), "Enter must not run the command");
  await app.chord(KEY.control, "c");
});

test(`${shell}: Tab inside vim goes to vim`, { skip }, async () => {
  await start(true);
  const out = join(dir, `vim-${shell}.txt`);
  await app.type(`vi -u NONE -N${KEY.enter}`);
  await sleep(1500);
  await app.type(`i${KEY.tab}x${KEY.escape}`);
  await sleep(300);
  await app.type(`:w! ${out}${KEY.enter}:q!${KEY.enter}`);
  await until("vim to write the file", () => {
    try {
      return readFileSync(out, "utf8");
    } catch {
      return false;
    }
  });
  assert.equal(readFileSync(out, "utf8"), "\tx\n");
  assert.equal(await menuOpen(), false);
});

test(`${shell}: disabled leaves the shell's own Tab`, { skip }, async () => {
  await start(false);
  await app.type("ls fixture-o" + KEY.tab);
  await app.waitFor(/ls fixture-one\.txt/);
  assert.equal(await menuOpen(), false);
  await app.chord(KEY.control, "c");
});
