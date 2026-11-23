import { describe, expect, it } from "vitest";

import type { OfficeEvent } from "../transport/office";
import { DEMO, playDemo, type DemoStep, type DemoTimers } from "./demo";

class Timers implements DemoTimers {
  queue: { callback: () => void; ms: number }[] = [];
  set(callback: () => void, ms: number): unknown {
    const entry = { callback, ms };
    this.queue.push(entry);
    return entry;
  }
  clear(handle: unknown): void {
    this.queue = this.queue.filter((e) => e !== handle);
  }
  fire(): number {
    const entry = this.queue.shift();
    entry?.callback();
    return entry?.ms ?? -1;
  }
}

const steps: DemoStep[] = [
  { after: 100, event: { v: 1, session: "s", agent: "main", kind: "start" } },
  { after: 200, event: { v: 1, session: "s", agent: "main", kind: "end" } },
];

describe("playDemo", () => {
  it("plays the steps in order, with the waits divided by the speed", () => {
    const timers = new Timers();
    const seen: OfficeEvent[] = [];
    playDemo((e) => seen.push(e), { steps, speed: 2, timers });
    expect(timers.fire()).toBe(50);
    expect(timers.fire()).toBe(100);
    expect(seen.map((e) => e.kind)).toEqual(["start", "end"]);
    expect(timers.queue).toHaveLength(0);
  });

  it("loops when asked and stops when told", () => {
    const timers = new Timers();
    const seen: OfficeEvent[] = [];
    const stop = playDemo((e) => seen.push(e), { steps, loop: true, timers });
    timers.fire();
    timers.fire();
    timers.fire();
    expect(seen).toHaveLength(3);
    stop();
    expect(timers.queue).toHaveLength(0);
  });

  it("the recorded demo uses the office format only", () => {
    for (const step of DEMO) {
      expect(step.event.v).toBe(1);
      expect(["start", "tool", "message", "wait", "end", "error", "task", "session_end"]).toContain(step.event.kind);
    }
  });
});
