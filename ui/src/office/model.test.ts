import { describe, expect, it } from "vitest";

import type { OfficeEvent, OfficeSnapshot } from "../transport/office";
import { DEMO } from "./demo";
import { breakSpot, inCorridor } from "./layout";
import { BUBBLE_MS, LINGER_MS, MAX_AGENTS, OfficeModel, type CharacterView } from "./model";

const SIZE = { width: 320, height: 600 };
const ev = (agent: string, fields: Partial<OfficeEvent>): OfficeEvent => ({ v: 1, session: "s1", agent, kind: "tool", ...fields });
const snap = (sessions: OfficeSnapshot["sessions"]): OfficeSnapshot => ({ v: 1, sessions });

/** Advances `ms` in 50 ms frames. */
function run(model: OfficeModel, ms: number): void {
  for (let t = 0; t < ms; t += 50) model.step(50);
}

const who = (model: OfficeModel, id: string, session = "s1"): CharacterView => {
  const c = model.characters().find((v) => v.id === id && v.session === session);
  if (!c) throw new Error(`${id} is not in the scene`);
  return c;
};

const seatOf = (model: OfficeModel, index: number, zone = 0) => model.layout.seats[zone]?.[index];
const at = (c: CharacterView) => ({ x: Math.round(c.x), y: Math.round(c.y) });
const round = (p: { x: number; y: number } | undefined) => p && { x: Math.round(p.x), y: Math.round(p.y) };

describe("OfficeModel: arriving and sitting", () => {
  it("a new agent comes in by the door and walks to its desk", () => {
    const model = new OfficeModel(SIZE);
    model.event(ev("main", { kind: "start" }));
    const c = who(model, "main");
    expect(at(c)).toEqual(round(model.layout.door));
    expect(c.walking).toBe(true);
    expect(c.place).toBe("desk");
    run(model, 10_000);
    const seated = who(model, "main");
    expect(seated.walking).toBe(false);
    expect(at(seated)).toEqual(round(seatOf(model, 0)));
  });

  it("keeps each desk with its owner, main first, others in order of arrival", () => {
    const model = new OfficeModel(SIZE);
    model.event(ev("a-ana", { kind: "start", name: "ana" }));
    model.event(ev("main", { kind: "start" }));
    model.event(ev("a-bruno", { kind: "start", name: "bruno" }));
    run(model, 10_000);
    expect(at(who(model, "main"))).toEqual(round(seatOf(model, 0)));
    expect(at(who(model, "a-ana"))).toEqual(round(seatOf(model, 1)));
    expect(at(who(model, "a-bruno"))).toEqual(round(seatOf(model, 2)));
    expect(who(model, "a-ana").label).toBe("ana");
  });

  it("an event from an unknown agent brings it in too", () => {
    const model = new OfficeModel(SIZE);
    model.event(ev("a1", { kind: "tool", action: "reading", target: "x.ts" }));
    expect(who(model, "a1").walking).toBe(true);
    expect(who(model, "a1").bubble?.text).toBe("reading x.ts");
  });
});

describe("OfficeModel: working, waiting, free", () => {
  it("follows the activity and bubble of each event, like core::state", () => {
    const model = new OfficeModel(SIZE);
    model.event(ev("main", { kind: "start" }));
    model.event(ev("main", { kind: "tool", action: "reading", target: "api.ts" }));
    expect(model.agents()).toEqual([{ session: "s1", agents: [{ id: "main", activity: "working", bubble: "reading api.ts" }] }]);
    model.event(ev("main", { kind: "wait", action: "permission", target: "Bash" }));
    expect(who(model, "main").activity).toBe("waiting");
    expect(who(model, "main").bubble?.glyph).toBe("?");
    model.event(ev("main", { kind: "error", action: "Bash", result: "failed" }));
    expect(who(model, "main").activity).toBe("waiting");
    expect(who(model, "main").cue).toBe("shake");
  });

  it("a finished subagent lingers at its desk, then goes for coffee", () => {
    const model = new OfficeModel(SIZE);
    model.event(ev("main", { kind: "start" }));
    model.event(ev("a1", { kind: "start", name: "ana" }));
    run(model, 10_000);
    model.event(ev("a1", { kind: "end", result: "done" }));
    expect(who(model, "a1").activity).toBe("idle");
    expect(who(model, "a1").bubble?.text).toBe("done");
    run(model, LINGER_MS - 100);
    expect(who(model, "a1").place).toBe("desk");
    expect(who(model, "a1").walking).toBe(false);
    run(model, 200);
    expect(who(model, "a1").place).toBe("coffee");
    run(model, 10_000);
    const c = who(model, "a1");
    expect(c.walking).toBe(false);
    expect(inCorridor(model.layout, c)).toBe(true);
    expect(at(c)).toEqual(round(breakSpot(model.layout, 0)));
    // Still in the office, as in the core's picture.
    expect(model.agents()[0]?.agents.map((a) => a.id)).toEqual(["main", "a1"]);
  });

  it("comes back to its desk when it works again", () => {
    const model = new OfficeModel(SIZE);
    model.event(ev("a1", { kind: "start" }));
    model.event(ev("a1", { kind: "end" }));
    run(model, 20_000);
    expect(who(model, "a1").place).toBe("coffee");
    model.event(ev("a1", { kind: "tool", action: "writing", target: "x.ts" }));
    expect(who(model, "a1").place).toBe("desk");
    run(model, 20_000);
    expect(at(who(model, "a1"))).toEqual(round(seatOf(model, 0)));
  });

  it("the main agent stays at its desk between turns", () => {
    const model = new OfficeModel(SIZE);
    model.event(ev("main", { kind: "start" }));
    model.event(ev("main", { kind: "end" }));
    run(model, 20_000);
    const c = who(model, "main");
    expect(c.activity).toBe("idle");
    expect(c.place).toBe("desk");
    expect(model.busy()).toBe(false);
  });

  it("when the session ends, everyone leaves by the door and the session goes", () => {
    const model = new OfficeModel(SIZE);
    model.event(ev("main", { kind: "start" }));
    model.event(ev("a1", { kind: "start" }));
    run(model, 10_000);
    // core::state on SessionEnd: an end for each agent, main last, with the reason.
    model.event(ev("a1", { kind: "end", result: "other" }));
    model.event(ev("main", { kind: "end", result: "other" }));
    expect(who(model, "main").place).toBe("out");
    expect(who(model, "a1").place).toBe("out");
    expect(model.agents()).toEqual([]);
    run(model, 20_000);
    expect(model.characters()).toEqual([]);
    expect(model.busy()).toBe(false);
  });
});

describe("OfficeModel: pictures", () => {
  const picture = snap([
    {
      session: "s1",
      pane: "p1",
      agents: [
        { id: "main", activity: "working", bubble: "reading api.ts" },
        { id: "a1", name: "ana", activity: "waiting", bubble: "permission: Bash" },
        { id: "a2", name: "bruno", activity: "idle", bubble: "12 tests pass" },
      ],
      tasks: [],
    },
  ]);

  it("places everyone at once, where they belong, and agrees with the picture", () => {
    const model = new OfficeModel(SIZE);
    model.snapshot(picture);
    expect(model.characters().every((c) => !c.walking)).toBe(true);
    expect(at(who(model, "main"))).toEqual(round(seatOf(model, 0)));
    expect(at(who(model, "a1"))).toEqual(round(seatOf(model, 1)));
    expect(who(model, "a2").place).toBe("coffee");
    expect(model.agents()).toEqual([
      {
        session: "s1",
        agents: (picture.sessions[0]?.agents ?? []).map((a) => ({
          id: a.id,
          ...(a.name ? { name: a.name } : {}),
          activity: a.activity,
          bubble: a.bubble,
        })),
      },
    ]);
  });

  it("whoever is gone from a new picture walks out", () => {
    const model = new OfficeModel(SIZE);
    model.snapshot(picture);
    model.snapshot(snap([{ session: "s1", agents: [{ id: "main", activity: "idle" }], tasks: [] }]));
    expect(who(model, "a1").place).toBe("out");
    expect(model.agents()[0]?.agents.map((a) => a.id)).toEqual(["main"]);
    run(model, 20_000);
    expect(model.characters().map((c) => c.id)).toEqual(["main"]);
    model.snapshot(snap([]));
    run(model, 20_000);
    expect(model.characters()).toEqual([]);
  });
});

describe("OfficeModel: bubbles, size and limits", () => {
  it("a bubble goes away after BUBBLE_MS, but waiting stays until the next change", () => {
    const model = new OfficeModel(SIZE);
    model.event(ev("main", { kind: "end" }));
    model.event(ev("main", { kind: "task", action: "created", text: "x" }));
    expect(who(model, "main").bubbleVisible).toBe(true);
    run(model, BUBBLE_MS + 100);
    expect(who(model, "main").bubbleVisible).toBe(false);
    model.event(ev("main", { kind: "wait", action: "user" }));
    run(model, BUBBLE_MS * 3);
    expect(who(model, "main").bubbleVisible).toBe(true);
  });

  it("keeps a short history of bubbles", () => {
    const model = new OfficeModel(SIZE);
    for (let i = 0; i < 30; i++) model.event(ev("main", { action: "reading", target: `f${i}.ts` }));
    const history = who(model, "main").history;
    expect(history).toHaveLength(20);
    expect(history.at(-1)?.text).toBe("reading f29.ts");
  });

  it("on resize, everyone is placed at once at the new spots", () => {
    const model = new OfficeModel(SIZE);
    model.event(ev("main", { kind: "start" }));
    run(model, 300);
    model.resize({ width: 640, height: 400 });
    const c = who(model, "main");
    expect(c.walking).toBe(false);
    expect(at(c)).toEqual(round(seatOf(model, 0)));
  });

  it("never holds more than MAX_AGENTS per session, keeping main", () => {
    const model = new OfficeModel(SIZE);
    model.event(ev("main", { kind: "start" }));
    for (let i = 0; i < MAX_AGENTS + 5; i++) model.event(ev(`a${i}`, { kind: "start" }));
    const ids = model.characters().map((c) => c.id);
    expect(ids).toHaveLength(MAX_AGENTS);
    expect(ids).toContain("main");
  });

  it("draws nearer characters last", () => {
    const model = new OfficeModel(SIZE);
    model.snapshot(snap([{ session: "s1", agents: ["main", "a", "b", "c"].map((id) => ({ id, activity: "working" as const })), tasks: [] }]));
    const ys = model.characters().map((c) => c.y);
    expect([...ys].sort((a, b) => a - b)).toEqual(ys);
  });
});

describe("OfficeModel: the demo, a session with two subagents", () => {
  it("animates from start to end", () => {
    const model = new OfficeModel(SIZE);
    let seenBoth = false;
    let waited = false;
    for (const step of DEMO) {
      run(model, step.after);
      model.event(step.event);
      const ids = model.agents()[0]?.agents.map((a) => a.id) ?? [];
      if (ids.includes("a-ana") && ids.includes("a-bruno")) seenBoth = true;
      if (model.characters().some((c) => c.activity === "waiting" && c.id === "a-bruno")) waited = true;
    }
    expect(seenBoth).toBe(true);
    expect(waited).toBe(true);
    run(model, 30_000);
    expect(model.characters()).toEqual([]);
    expect(model.busy()).toBe(false);
  });
});
