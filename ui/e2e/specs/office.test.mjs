// Phase 2, tasks B1, D3 and B2 (docs/FASE2-PLANO.md): an event sent by
// termbay-hook from inside a pane goes through the event server and the
// office state, opens the office panel by itself (decision E9) and shows in
// the office debug list (the e2e build has VITE_OFFICE_DEBUG=1). The
// shortcut closes the panel, and a closed panel draws nothing.
//
// Environment: TERMBAY_E2E_HOOK, path to the termbay-hook binary.
import assert from "node:assert/strict";
import { afterEach, test } from "node:test";

import { App, KEY, isWindows, sleep, until, writeConfig } from "../driver.mjs";

let app;
afterEach(async () => app?.quit());

test("an event sent by termbay-hook in a pane shows in the office", async () => {
  const hook = process.env.TERMBAY_E2E_HOOK;
  assert.ok(hook, "TERMBAY_E2E_HOOK is not set");
  writeConfig("");
  app = await App.start();
  assert.equal(await app.isVisible("#office-panel"), false, "no agent yet: the panel is closed");

  // The shell has TERMBAY_EVENTS, TERMBAY_TOKEN and TERMBAY_PANE (task A2).
  const payload =
    '{"session_id":"e2e","hook_event_name":"PreToolUse","tool_name":"Read","tool_input":{"file_path":"/tmp/projeto/E2E_OFFICE.ts"}}';
  const command = isWindows ? `'${payload}' | & '${hook}' PreToolUse` : `printf '%s' '${payload}' | '${hook}' PreToolUse`;
  await app.type(command + KEY.enter);

  const line = await until(
    "the event in the office debug list",
    () =>
      app.run(`
        const item = [...document.querySelectorAll("#office-panel .office-debug li")]
          .find((li) => li.textContent.includes("E2E_OFFICE.ts"));
        return item ? item.textContent : null;
      `),
    20000,
  );
  // Reduced by the hook (the file name, never the folder), then turned
  // into an office event by core::state: the main agent is reading.
  assert.match(line, /^tool · main · reading · E2E_OFFICE\.ts$/);
  assert.ok(await app.isVisible("#office-panel"), "the panel opens on the first event");
  assert.ok(await app.isVisible(".office-divider"), "with its divider");

  // Ctrl+Shift+O (toggleOffice) closes it; the terminal keeps working.
  await app.focusPane();
  await app.chord(KEY.control, KEY.shift, "o");
  await until("the panel to close", async () => !(await app.isVisible("#office-panel")), 5000);
  assert.equal(await app.isVisible(".office-divider"), false);

  // Closed and idle, nothing asks for animation frames. First let the
  // terminals refit to the freed width, and blur them so the blinking
  // cursor does not count.
  await sleep(1500);
  await app.run(`
    document.activeElement?.blur();
    window.__rafCount = 0;
    const raf = window.requestAnimationFrame.bind(window);
    window.requestAnimationFrame = (cb) => { window.__rafCount++; return raf(cb); };
  `);
  await sleep(1500);
  assert.equal(await app.run("return window.__rafCount;"), 0, "a closed panel requests no frames");

  // More agent events do not reopen it (only the shortcut or the button do).
  await app.focusPane();
  await app.type(command + KEY.enter);
  await sleep(1500);
  assert.equal(await app.isVisible("#office-panel"), false, "later events leave the panel closed");
});

test("with auto_open off, the panel waits for the shortcut", async () => {
  const hook = process.env.TERMBAY_E2E_HOOK;
  assert.ok(hook, "TERMBAY_E2E_HOOK is not set");
  writeConfig("[office]\nauto_open = false\n");
  app = await App.start();

  const payload =
    '{"session_id":"e2e-off","hook_event_name":"PreToolUse","tool_name":"Read","tool_input":{"file_path":"/tmp/projeto/E2E_OFF.ts"}}';
  const command = isWindows ? `'${payload}' | & '${hook}' PreToolUse` : `printf '%s' '${payload}' | '${hook}' PreToolUse`;
  await app.type(command + KEY.enter);
  await sleep(2000);
  assert.equal(await app.isVisible("#office-panel"), false);

  await app.focusPane();
  await app.chord(KEY.control, KEY.shift, "o");
  const line = await until(
    "the event in the office debug list",
    () =>
      app.run(`
        const item = [...document.querySelectorAll("#office-panel .office-debug li")]
          .find((li) => li.textContent.includes("E2E_OFF.ts"));
        return item ? item.textContent : null;
      `),
    10000,
  );
  // Held by the panel until the first opening, then handed to the content.
  assert.match(line, /^tool · main · reading · E2E_OFF\.ts$/);
});
