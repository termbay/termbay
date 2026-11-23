import { describe, expect, it } from "vitest";

import { FakeClock } from "./fake-clock.test-util";
import { FrameLoop, MAX_FPS, MAX_STEP_MS } from "./loop";

function setup(more: () => boolean = () => true) {
  const clock = new FakeClock();
  const steps: number[] = [];
  const loop = new FrameLoop((dt) => {
    steps.push(dt);
    return more();
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

  it("stops asking for frames when nothing moves", () => {
    let frames = 3;
    const { clock, steps, loop } = setup(() => --frames > 0);
    loop.wake();
    clock.run(20);
    expect(steps).toHaveLength(3);
    expect(loop.running).toBe(false);
    expect(clock.pending).toBe(0);
  });

  it("setVisible(false) cancels the pending frame and asks for none", () => {
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
