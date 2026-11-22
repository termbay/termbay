// @vitest-environment happy-dom
import { describe, expect, it } from "vitest";

import { subscribeOffice, type OfficeEvent, type OfficeIpc, type OfficeSnapshot } from "../transport/office";
import { DEBUG_LINES, OfficeDebugList, formatAgent, formatEvent } from "./debug";

const event = (fields: Partial<OfficeEvent>): OfficeEvent => ({ v: 1, session: "s1", agent: "main", kind: "tool", ...fields });
const snapshot = (sessions: OfficeSnapshot["sessions"]): OfficeSnapshot => ({ v: 1, sessions });

describe("formatEvent", () => {
  it("shows the kind, who and what", () => {
    expect(formatEvent(event({ action: "reading", target: "api.ts" }))).toBe("tool · main · reading · api.ts");
    expect(formatEvent(event({ agent: "a1", kind: "start", name: "bruno", role: "qa", color: "green" }))).toBe(
      "start · bruno · qa · green",
    );
    expect(formatEvent(event({ agent: "a1", kind: "end", result: "pronto" }))).toBe("end · a1 · pronto");
  });

  it("uses names seen before, for the agent and the recipient", () => {
    const names = new Map([
      ["s1\u0000a1", "ana"],
      ["s1\u0000a2", "bruno"],
    ]);
    expect(formatEvent(event({ agent: "a1", kind: "message", to: "a2", text: "README" }), names)).toBe("message · ana · → bruno · README");
  });
});

describe("formatAgent", () => {
  it("shows name, activity and bubble", () => {
    expect(formatAgent({ id: "main", activity: "working", bubble: "reading api.ts" })).toBe("here · main · working · reading api.ts");
    expect(formatAgent({ id: "a1", name: "ana", activity: "idle" })).toBe("here · ana · idle");
  });
});

describe("OfficeDebugList", () => {
  it("lists the picture, then the changes with the names it learned", () => {
    const panel = document.createElement("aside");
    panel.hidden = true;
    const list = new OfficeDebugList(panel);
    // An empty office does not open the panel.
    list.snapshot(snapshot([]));
    expect(panel.hidden).toBe(true);

    list.snapshot(snapshot([{ session: "s1", agents: [{ id: "a1", name: "ana", activity: "working" }], tasks: [] }]));
    list.event(event({ agent: "a1", action: "reading", target: "x.ts" }));
    expect(panel.hidden).toBe(false);
    expect([...panel.querySelectorAll("li")].map((li) => li.textContent)).toEqual(["here · ana · working", "tool · ana · reading · x.ts"]);
  });

  it("sets text, never HTML, and keeps the last lines", () => {
    const panel = document.createElement("aside");
    const list = new OfficeDebugList(panel);
    list.event(event({ target: "<img src=x onerror=alert(1)>" }));
    expect(panel.querySelector("img")).toBeNull();

    for (let i = 0; i < DEBUG_LINES + 10; i++) list.event(event({ target: `T${i}` }));
    const items = panel.querySelectorAll("li");
    expect(items.length).toBe(DEBUG_LINES);
    expect(items[items.length - 1]?.textContent).toContain(`T${DEBUG_LINES + 9}`);

    list.toggle();
    expect(panel.hidden).toBe(true);
  });
});

describe("subscribeOffice", () => {
  it("gives the picture first, then the changes", async () => {
    let push: ((e: OfficeEvent) => void) | undefined;
    const calls: Array<[string, Record<string, unknown>]> = [];
    const ipc: OfficeIpc = {
      channel: (onMessage) => {
        push = onMessage;
        return "channel";
      },
      invoke: async <T>(cmd: string, args: Record<string, unknown>) => {
        calls.push([cmd, args]);
        return snapshot([]) as T;
      },
    };
    const seen: string[] = [];
    await subscribeOffice(ipc, { snapshot: () => seen.push("snapshot"), event: (e) => seen.push(e.kind) });
    push?.(event({ kind: "end" }));
    expect(calls).toEqual([["office_subscribe", { events: "channel" }]]);
    expect(seen).toEqual(["snapshot", "end"]);
  });

  it("holds a change that arrives before the picture", async () => {
    let push: ((e: OfficeEvent) => void) | undefined;
    const ipc: OfficeIpc = {
      channel: (onMessage) => {
        push = onMessage;
        return "channel";
      },
      invoke: async <T>() => {
        push?.(event({ kind: "start" }));
        return snapshot([]) as T;
      },
    };
    const seen: string[] = [];
    await subscribeOffice(ipc, { snapshot: () => seen.push("snapshot"), event: (e) => seen.push(e.kind) });
    expect(seen).toEqual(["snapshot", "start"]);
  });
});
