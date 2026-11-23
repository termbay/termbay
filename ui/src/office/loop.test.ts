import { describe, expect, it } from "vitest";

import { FakeClock } from "./fake-clock.test-util";
import { FrameLoop, MAX_FPS, MAX_STEP_MS } from "./loop";

function setup(next: () => number = () => 0) {
  const clock = new FakeClock();
  const steps: number[] = [];
  const loop = new FrameLoop((dt) => {
    steps.push(dt);
    return next();
  }, clock);
  return { clock, steps, loop };
}

describe("FrameLoop", () => {
  it("does nothing until woken", () => {
    const { clock, loop } = setup();
    expect(loop.running).toBe(false);
    expect(clock.requests).toBe(0);
  });

  it("draws a change at once, then at most MAX_FPS frames a second", () => {
    const { clock, steps, loop } = setup();
    loop.wake();
    clock.tick();
    expect(steps).toEqual([0]);
    clock.run(60);
    // About one second of display frames at 60 Hz: MAX_FPS frames.
    expect(steps.length - 1).toBeGreaterThanOrEqual(MAX_FPS - 1);
    expect(steps.length - 1).toBeLessThanOrEqual(MAX_FPS);
    for (const dt of steps.slice(1)) expect(dt).toBeGreaterThanOrEqual(1000 / MAX_FPS - 2);
  });

  it("stops asking for frames when nothing will change", () => {
    let frames = 3;
    const { clock, steps, loop } = setup(() => (--frames > 0 ? 0 : Number.POSITIVE_INFINITY));
    loop.wake();
    clock.run(20);
    expect(steps).toHaveLength(3);
    expect(loop.running).toBe(false);
    expect(loop.sleeping).toBe(false);
    expect(clock.pending).toBe(0);
  });

  it("sleeps on a timer until a change far ahead, with no frames meanwhile", () => {
    const { clock, steps, loop } = setup(() => 400);
    loop.wake();
    clock.tick();
    expect(loop.running).toBe(false);
    expect(loop.sleeping).toBe(true);
    const requests = clock.requests;
    // One minute at 60 Hz: one frame every 400 ms, not 30 a second.
    clock.run(3600);
    expect(steps.length).toBeGreaterThanOrEqual(140);
    expect(steps.length).toBeLessThanOrEqual(155);
    expect(clock.requests - requests).toBeLessThanOrEqual(155);
    // The step after a sleep covers the sleep, so animations keep their pace.
    for (const dt of steps.slice(1)) expect(dt).toBeGreaterThanOrEqual(400);
  });

  it("a change during a sleep is drawn at once", () => {
    const { clock, steps, loop } = setup(() => 5000);
    loop.wake();
    clock.tick();
    loop.wake();
    expect(loop.sleeping).toBe(false);
    clock.tick();
    expect(steps).toHaveLength(2);
  });

  it("setVisible(false) cancels the pending frame and timer and asks for none", () => {
    const { clock, steps, loop } = setup();
    loop.wake();
    clock.run(5);
    loop.setVisible(false);
    expect(loop.running).toBe(false);
    expect(clock.pending).toBe(0);
    const before = clock.requests;
    const drawn = steps.length;
    // Changes while hidden ask for nothing.
    loop.wake();
    loop.wake();
    clock.run(30);
    expect(clock.requests).toBe(before);
    expect(steps).toHaveLength(drawn);

    const sleepy = setup(() => 1000);
    sleepy.loop.wake();
    sleepy.clock.tick();
    expect(sleepy.clock.pendingTimers).toBe(1);
    sleepy.loop.setVisible(false);
    expect(sleepy.clock.pendingTimers).toBe(0);
    expect(sleepy.clock.pending).toBe(0);
  });

  it("starts again when shown, without counting the hidden time", () => {
    const { clock, steps, loop } = setup();
    loop.wake();
    clock.run(3);
    loop.setVisible(false);
    clock.now += 60_000;
    loop.setVisible(true);
    expect(loop.running).toBe(true);
    clock.tick();
    expect(steps.at(-1)).toBe(0);
  });

  it("stops while the page is hidden, even with the panel visible", () => {
    const { clock, loop } = setup();
    loop.wake();
    loop.setPageHidden(true);
    expect(clock.pending).toBe(0);
    loop.wake();
    expect(clock.pending).toBe(0);
    loop.setPageHidden(false);
    expect(clock.pending).toBe(1);
  });

  it("caps a long frame so nobody teleports", () => {
    const { clock, steps, loop } = setup();
    loop.wake();
    clock.tick();
    clock.tick(5000);
    expect(steps.at(-1)).toBe(MAX_STEP_MS);
  });

  it("never runs again after destroy", () => {
    const { clock, loop } = setup();
    loop.wake();
    loop.destroy();
    expect(clock.pending).toBe(0);
    loop.wake();
    loop.setVisible(true);
    expect(clock.pending).toBe(0);
  });
});
