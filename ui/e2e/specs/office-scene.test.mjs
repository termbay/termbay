// Phase 2, task C2 (docs/FASE2-PLANO.md): the office scene draws in the
// panel. After the first agent event the scene's canvas exists, and after a
// subagent arrives, works and finishes, the scene's agents match the core's
// picture of the office (core::state). The e2e build exposes
// window.__termbayOffice (src/office/e2e-content.ts) for this; release
// builds never contain it (e2e/check-release.mjs).
//
// Environment: TERMBAY_E2E_HOOK, path to the termbay-hook binary.
import assert from "node:assert/strict";
import { afterEach, test } from "node:test";

import { App, KEY, isWindows, until, writeConfig } from "../driver.mjs";

let app;
afterEach(async () => app?.quit());

/** Types a command that sends `payload` to termbay-hook as `event`, from the pane. */
async function send(hook, event, payload) {
  const json = JSON.stringify({ session_id: "e2e-scene", hook_event_name: event, ...payload });
  const command = isWindows ? `'${json}' | & '${hook}' ${event}` : `printf '%s' '${json}' | '${hook}' ${event}`;
  await app.focusPane();
  await app.type(command + KEY.enter);
}

/** The scene's agents and the core's picture, both from the page. */
async function scene() {
  return app.run("return JSON.stringify(window.__termbayOffice ? window.__termbayOffice.scene() : null);");
}

async function core() {
  await app.run(`
    window.__e2eCore = null;
    window.__termbayOffice.core().then((c) => { window.__e2eCore = JSON.stringify(c); });
  `);
  return until("the core's picture", () => app.run("return window.__e2eCore;"), 10000);
}

test("the scene draws after the first agent event and matches the core's picture", async () => {
  const hook = process.env.TERMBAY_E2E_HOOK;
  assert.ok(hook, "TERMBAY_E2E_HOOK is not set");
  writeConfig("");
  app = await App.start();
  assert.equal(await app.run("return document.querySelector('#office-panel canvas') !== null;"), false);

  await send(hook, "PreToolUse", { tool_name: "Read", tool_input: { file_path: "/tmp/projeto/E2E_SCENE.ts" } });
  await until(
    "the scene's canvas in the office panel",
    () => app.run("return !!window.__termbayOffice && window.__termbayOffice.canvas() && !!document.querySelector('#office-panel canvas');"),
    20000,
  );
  assert.ok(await app.isVisible("#office-panel canvas"), "the canvas is shown");

  // A subagent arrives, searches and finishes with a report.
  await send(hook, "SubagentStart", { agent_id: "e2e-a1", agent_type: "qa" });
  await send(hook, "PreToolUse", { agent_id: "e2e-a1", agent_type: "qa", tool_name: "Grep", tool_input: { pattern: "TODO" } });
  await send(hook, "SubagentStop", { agent_id: "e2e-a1", agent_type: "qa", last_assistant_message: "found 3" });

  const expected = [
    {
      session: "e2e-scene",
      agents: [
        { id: "main", activity: "working", bubble: "reading E2E_SCENE.ts" },
        { id: "e2e-a1", activity: "idle", bubble: "found 3" },
      ],
    },
  ];
  // The scene catches up with the events; then it must agree with the core.
  const seen = await until(
    "the scene to show the finished subagent",
    async () => {
      const s = JSON.parse((await scene()) ?? "null");
      const session = s?.find((x) => x.session === "e2e-scene");
      return session?.agents.some((a) => a.id === "e2e-a1" && a.activity === "idle") ? s : null;
    },
    20000,
  );
  const picture = JSON.parse(await core());
  const only = (sessions) => sessions.filter((s) => s.session === "e2e-scene");
  assert.deepEqual(only(seen), only(picture), "the scene matches the core's picture");
  assert.deepEqual(only(picture), expected);
});
