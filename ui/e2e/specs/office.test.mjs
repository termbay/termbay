// Phase 2, tasks B1 and D3 (docs/FASE2-PLANO.md): an event sent by
// termbay-hook from inside a pane goes through the event server and the
// office state and shows in the office debug list (the e2e build has
// VITE_OFFICE_DEBUG=1).
//
// Environment: TERMBAY_E2E_HOOK, path to the termbay-hook binary.
import assert from "node:assert/strict";
import { afterEach, test } from "node:test";

import { App, KEY, isWindows, until, writeConfig } from "../driver.mjs";

let app;
afterEach(async () => app?.quit());

test("an event sent by termbay-hook in a pane shows in the office", async () => {
  const hook = process.env.TERMBAY_E2E_HOOK;
  assert.ok(hook, "TERMBAY_E2E_HOOK is not set");
  writeConfig("");
  app = await App.start();

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
});
