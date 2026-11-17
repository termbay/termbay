// Milestone 1 end to end: the app opens, a command typed in the terminal
// runs in the shell and its output shows up. Fails when the pty does not answer.
import assert from "node:assert/strict";
import { after, before, test } from "node:test";

import { App, say, writeConfig } from "../driver.mjs";

let app;
before(async () => {
  writeConfig("");
  app = await App.start();
});
after(async () => app?.quit());

test("a command typed in the terminal runs and its output shows", async () => {
  await app.runCommand(say("TERMBAY", "_E2E_OK"), /TERMBAY_E2E_OK/);
  const text = await app.focusedText();
  assert.match(text, /^TERMBAY_E2E_OK$/m, "the output is on its own line, not just in the echo");
});
