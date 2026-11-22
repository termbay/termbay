// @vitest-environment happy-dom
import { describe, expect, it } from "vitest";

import { subscribeOffice, type OfficeIpc, type ReducedEvent } from "../transport/office";
import { DEBUG_LINES, OfficeDebugList, formatEvent } from "./debug";

const event = (fields: Partial<ReducedEvent>): ReducedEvent => ({ v: 1, event: "PreToolUse", session: "s1", ...fields });

describe("formatEvent", () => {
  it("names the main agent, a subagent and a teammate", () => {
    expect(formatEvent(event({ tool: "Read", file: "api.ts" }))).toBe("PreToolUse · main · Read · api.ts");
    expect(formatEvent(event({ agent_id: "a1", agent_type: "bruno-teste", tool: "Bash", program: "npm", description: "Run tests" }))).toBe(
      "PreToolUse · bruno-teste · Bash · npm · Run tests",
    );
    expect(formatEvent(event({ event: "TeammateIdle", agent_id: "a1", agent_type: "x", teammate_name: "ana" }))).toBe("TeammateIdle · ana");
    expect(formatEvent(event({ agent_id: "a1" }))).toBe("PreToolUse · agent");
  });

  it("shows messages and results", () => {
    expect(formatEvent(event({ tool: "SendMessage", to: "bruno", summary: "README" }))).toBe("PreToolUse · main · SendMessage · → bruno · README");
    expect(formatEvent(event({ event: "SubagentStop", agent_id: "a1", agent_type: "qa", last_message: "ok" }))).toBe("SubagentStop · qa · ok");
  });
});

describe("OfficeDebugList", () => {
  it("adds text lines, keeps the last ones and shows the panel", () => {
    const panel = document.createElement("aside");
    panel.hidden = true;
    const list = new OfficeDebugList(panel);
    list.add(event({ tool: "Read", file: "<img src=x onerror=alert(1)>" }));
    expect(panel.hidden).toBe(false);
    expect(panel.querySelector("img")).toBeNull();
    expect(panel.querySelector("li")?.textContent).toContain("<img");

    for (let i = 0; i < DEBUG_LINES + 10; i++) list.add(event({ tool: `T${i}` }));
    const items = panel.querySelectorAll("li");
    expect(items.length).toBe(DEBUG_LINES);
    expect(items[items.length - 1]?.textContent).toContain(`T${DEBUG_LINES + 9}`);

    list.toggle();
    expect(panel.hidden).toBe(true);
  });
});

describe("subscribeOffice", () => {
  it("delivers the recent events first, then the new ones", async () => {
    let push: ((e: ReducedEvent) => void) | undefined;
    const calls: Array<[string, Record<string, unknown>]> = [];
    const ipc: OfficeIpc = {
      channel: (onMessage) => {
        push = onMessage;
        return "channel";
      },
      invoke: async <T>(cmd: string, args: Record<string, unknown>) => {
        calls.push([cmd, args]);
        return [event({ session: "old" })] as T;
      },
    };
    const seen: string[] = [];
    await subscribeOffice(ipc, (e) => seen.push(e.session));
    push?.(event({ session: "new" }));
    expect(calls).toEqual([["office_subscribe", { events: "channel" }]]);
    expect(seen).toEqual(["old", "new"]);
  });

  it("holds an event that arrives before the recent ones are returned", async () => {
    let push: ((e: ReducedEvent) => void) | undefined;
    const ipc: OfficeIpc = {
      channel: (onMessage) => {
        push = onMessage;
        return "channel";
      },
      invoke: async <T>() => {
        push?.(event({ session: "early" }));
        return [event({ session: "old" })] as T;
      },
    };
    const seen: string[] = [];
    await subscribeOffice(ipc, (e) => seen.push(e.session));
    expect(seen).toEqual(["old", "early"]);
  });
});
