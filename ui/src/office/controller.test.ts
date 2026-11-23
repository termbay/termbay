import { describe, expect, it } from "vitest";

import { subscribeOffice, type OfficeEvent, type OfficeIpc, type OfficeSnapshot } from "../transport/office";
import { createOfficeScene, frameKey, type SceneFrame, type SceneHost, type SceneRenderer } from "./controller";
import { FakeClock } from "./fake-clock.test-util";
import type { Size } from "./layout";

class Recorder implements SceneRenderer {
  frames: SceneFrame[] = [];
  sizes: Size[] = [];
  destroyed = false;
  resize(size: Size): void {
    this.sizes.push(size);
  }
  draw(frame: SceneFrame): void {
    this.frames.push(frame);
  }
  destroy(): void {
    this.destroyed = true;
  }
}

const ev = (agent: string, fields: Partial<OfficeEvent>): OfficeEvent => ({ v: 1, session: "s1", agent, kind: "tool", ...fields });

function setup(extra: Partial<SceneHost> = {}) {
  const clock = new FakeClock();
  const renderer = new Recorder();
  let onSize: ((size: Size) => void) | null = null;
  let onHidden: ((hidden: boolean) => void) | null = null;
  let observers = 0;
  const host: SceneHost = {
    clock,
    size: { width: 320, height: 600 },
    observeSize(cb) {
      onSize = cb;
      observers++;
      return () => observers--;
    },
    observePage(cb) {
      onHidden = cb;
      observers++;
      return () => observers--;
    },
    ...extra,
  };
  const scene = createOfficeScene(renderer, host);
  return {
    clock,
    renderer,
    scene,
    resize: (size: Size) => onSize?.(size),
    pageHidden: (hidden: boolean) => onHidden?.(hidden),
    observers: () => observers,
  };
}

describe("createOfficeScene", () => {
  it("draws the first frame, then stops when nothing moves", () => {
    const { clock, renderer, scene } = setup();
    clock.tick();
    expect(renderer.frames).toHaveLength(1);
    expect(scene.loop.running).toBe(false);
    expect(clock.pending).toBe(0);
  });

  it("animates an event, and stops once everyone has settled", () => {
    const { clock, renderer, scene } = setup();
    scene.event(ev("main", { kind: "start" }));
    scene.event(ev("main", { kind: "end" }));
    clock.run(3);
    expect(renderer.frames.at(-1)?.characters.map((c) => c.id)).toEqual(["main"]);
    clock.run(60 * 30);
    expect(scene.loop.running).toBe(false);
    const drawn = renderer.frames.length;
    clock.run(100);
    expect(renderer.frames).toHaveLength(drawn);
  });

  it("setVisible(false) stops the ticker: no frame is requested while hidden", () => {
    const { clock, renderer, scene } = setup();
    scene.event(ev("main", { kind: "start" }));
    clock.run(5);
    expect(scene.loop.running).toBe(true);
    scene.setVisible(false);
    expect(clock.pending).toBe(0);
    const requests = clock.requests;
    const drawn = renderer.frames.length;
    // Events keep the state up to date, but ask for no frames.
    scene.event(ev("a1", { kind: "start", name: "ana" }));
    scene.event(ev("a1", { action: "reading", target: "api.ts" }));
    clock.run(120);
    expect(clock.requests).toBe(requests);
    expect(renderer.frames).toHaveLength(drawn);
    expect(scene.model.agents()[0]?.agents.map((a) => a.id)).toEqual(["main", "a1"]);

    scene.setVisible(true);
    clock.tick();
    expect(renderer.frames.at(-1)?.characters.find((c) => c.id === "a1")?.bubble?.text).toBe("reading api.ts");
  });

  it("stops while the page is hidden (window minimized)", () => {
    const { clock, scene, pageHidden } = setup();
    scene.event(ev("main", { kind: "start" }));
    pageHidden(true);
    expect(clock.pending).toBe(0);
    scene.event(ev("main", { action: "reading" }));
    expect(clock.pending).toBe(0);
    pageHidden(false);
    expect(clock.pending).toBe(1);
  });

  it("starts stopped when the page is already hidden", () => {
    const { clock } = setup({ pageHidden: true });
    expect(clock.pending).toBe(0);
  });

  it("follows the panel's size, ignoring the 0 x 0 of a hidden panel", () => {
    const { clock, renderer, resize } = setup();
    clock.tick();
    resize({ width: 0, height: 0 });
    expect(renderer.sizes).toEqual([{ width: 320, height: 600 }]);
    resize({ width: 640, height: 400 });
    expect(renderer.sizes.at(-1)).toEqual({ width: 640, height: 400 });
    clock.tick();
    expect(renderer.frames.at(-1)?.layout.size).toEqual({ width: 640, height: 400 });
  });

  it("destroy stops frames, observers and the renderer; later calls do nothing", () => {
    const { clock, renderer, scene, observers } = setup();
    scene.event(ev("main", { kind: "start" }));
    expect(observers()).toBe(2);
    scene.destroy();
    expect(renderer.destroyed).toBe(true);
    expect(clock.pending).toBe(0);
    expect(observers()).toBe(0);
    scene.event(ev("a1", { kind: "start" }));
    scene.setVisible(true);
    expect(clock.pending).toBe(0);
    scene.destroy();
  });

  it("a main agent waiting for hours draws only when its dots change, not at 30 fps", () => {
    const { clock, renderer, scene } = setup();
    scene.event(ev("main", { kind: "start" }));
    scene.event(ev("main", { kind: "wait", action: "user" }));
    // Let it walk to its desk and settle.
    clock.run(60 * 20);
    const draws = renderer.frames.length;
    const requests = clock.requests;
    // One minute at 60 Hz.
    clock.run(60 * 60);
    const minuteDraws = renderer.frames.length - draws;
    const minuteRequests = clock.requests - requests;
    // The dots change every THINK_FRAME_MS (400 ms): about 150 a minute.
    expect(minuteDraws).toBeGreaterThan(100);
    expect(minuteDraws).toBeLessThanOrEqual(160);
    expect(minuteRequests).toBeLessThanOrEqual(320);
    expect(scene.loop.sleeping || scene.loop.running).toBe(true);
    // Each drawn frame differs from the one before.
    const dots = renderer.frames.slice(draws).map((f) => f.characters[0]?.dots);
    for (let i = 1; i < dots.length; i++) expect(dots[i]).not.toBe(dots[i - 1]);
  });

  it("never draws a frame that looks like the last one", () => {
    const { clock, renderer, scene } = setup();
    scene.event(ev("main", { kind: "start" }));
    clock.run(60 * 20);
    const keys = renderer.frames.map((f) => frameKey(f));
    for (let i = 1; i < keys.length; i++) expect(keys[i]).not.toBe(keys[i - 1]);
    // A change that shows nothing new (same bubble again) draws nothing.
    scene.event(ev("main", { kind: "end" }));
    clock.run(60 * 20);
    const settled = renderer.frames.length;
    scene.snapshot({ v: 1, sessions: [{ session: "s1", agents: [{ id: "main", activity: "idle" }], tasks: [] }] });
    clock.run(10);
    expect(renderer.frames).toHaveLength(settled);
  });

  it("redraws after being shown again, even with nothing new", () => {
    const { clock, renderer, scene } = setup();
    clock.tick();
    const drawn = renderer.frames.length;
    scene.setVisible(false);
    scene.setVisible(true);
    clock.tick();
    expect(renderer.frames).toHaveLength(drawn + 1);
  });

  it("works as the listener of subscribeOffice", async () => {
    const { clock, renderer, scene } = setup();
    let send: (event: OfficeEvent) => void = () => {};
    const picture: OfficeSnapshot = {
      v: 1,
      sessions: [{ session: "s1", agents: [{ id: "main", activity: "working", bubble: "reading api.ts" }], tasks: [] }],
    };
    const ipc: OfficeIpc = {
      channel: (onMessage) => {
        send = onMessage;
        return {};
      },
      invoke: async <T>() => picture as T,
    };
    await subscribeOffice(ipc, scene);
    send(ev("a1", { kind: "start", name: "ana" }));
    clock.tick();
    expect(renderer.frames.at(-1)?.characters.map((c) => c.label).sort()).toEqual(["ana", "main"]);
  });
});
