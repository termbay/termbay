// @vitest-environment happy-dom
// PixiJS needs WebGL, so it is replaced here by small fakes that record what
// the renderer does with them: creation, drawing and, above all, cleanup.
import { beforeEach, describe, expect, it, vi } from "vitest";

const pixi = vi.hoisted(() => {
  class Point {
    x = 0;
    y = 0;
    set(x: number, y = x) {
      this.x = x;
      this.y = y;
    }
  }
  class Container {
    children: Container[] = [];
    destroyed = false;
    destroyOptions: unknown = undefined;
    visible = true;
    zIndex = 0;
    sortableChildren: boolean;
    position = new Point();
    scale = new Point();
    width = 10;
    height = 10;
    constructor(options: { sortableChildren?: boolean } = {}) {
      this.sortableChildren = options.sortableChildren ?? false;
    }
    get x() {
      return this.position.x;
    }
    get y() {
      return this.position.y;
    }
    addChild(...children: Container[]) {
      this.children.push(...children);
      return children[0];
    }
    destroy(options?: unknown) {
      this.destroyed = true;
      this.destroyOptions = options;
    }
  }
  class Graphics extends Container {
    clear() {
      return this;
    }
    rect() {
      return this;
    }
    roundRect() {
      return this;
    }
    poly() {
      return this;
    }
    circle() {
      return this;
    }
    fill() {
      return this;
    }
    stroke() {
      return this;
    }
  }
  class BitmapText extends Container {
    text = "";
    tint = 0;
    anchor = new Point();
    style: { wordWrapWidth?: number };
    constructor(options: { style?: { wordWrapWidth?: number } } = {}) {
      super();
      this.style = { ...options.style };
    }
  }
  const state = { failInit: false, apps: [] as Application[] };
  class Application {
    initOptions: Record<string, unknown> = {};
    canvas = document.createElement("canvas");
    stage = new Container();
    renderer = { resize: vi.fn() };
    render = vi.fn();
    stop = vi.fn();
    destroy = vi.fn();
    constructor() {
      state.apps.push(this);
    }
    async init(options: Record<string, unknown>) {
      this.initOptions = options;
      if (state.failInit) throw new Error("no WebGL");
    }
  }
  const Ticker = { system: { autoStart: true, stop: vi.fn(), update: vi.fn() } };
  return { Application, BitmapText, Container, Graphics, Ticker, state };
});

vi.mock("pixi.js", () => pixi);

import type { SceneFrame } from "./controller";
import { OfficeModel } from "./model";
import { PixiRenderer } from "./render";

const SIZE = { width: 320, height: 600 };

function frameWith(ids: string[]): SceneFrame {
  const model = new OfficeModel(SIZE);
  model.snapshot({
    v: 1,
    sessions: [{ session: "s1", agents: ids.map((id) => ({ id, name: id, activity: "working" as const, bubble: `doing ${id}` })), tasks: [] }],
  });
  return { layout: model.layout, characters: model.characters(), time: 0 };
}

/** Everything added to the stage, at any depth. */
function all(app: InstanceType<typeof pixi.Application>) {
  const out: InstanceType<typeof pixi.Container>[] = [];
  const walk = (c: InstanceType<typeof pixi.Container>) => {
    out.push(c);
    c.children.forEach(walk);
  };
  walk(app.stage);
  return out;
}

beforeEach(() => {
  pixi.state.failInit = false;
  pixi.state.apps.length = 0;
  pixi.Ticker.system.autoStart = true;
  vi.clearAllMocks();
});

describe("PixiRenderer", () => {
  it("creates a stopped application and puts its canvas in the container", async () => {
    const container = document.createElement("div");
    const renderer = await PixiRenderer.create(container, SIZE);
    const app = pixi.state.apps[0];
    expect(app?.initOptions).toMatchObject({ autoStart: false, sharedTicker: false, width: 320, height: 600 });
    expect(app?.stop).toHaveBeenCalled();
    expect(container.querySelector("canvas.office-canvas")).not.toBeNull();
    // PixiJS's global ticker cannot start a loop of its own while we live.
    expect(pixi.Ticker.system.autoStart).toBe(false);
    expect(pixi.Ticker.system.stop).toHaveBeenCalled();
    renderer.destroy();
  });

  it("draws by hand, advancing PixiJS's ticker only inside our frames", async () => {
    const renderer = await PixiRenderer.create(document.createElement("div"), SIZE);
    renderer.draw(frameWith(["main", "a1"]));
    expect(pixi.state.apps[0]?.render).toHaveBeenCalledTimes(1);
    expect(pixi.Ticker.system.update).toHaveBeenCalledTimes(1);
    renderer.destroy();
  });

  it("destroy frees the sprites and the application, removes the canvas, and gives the ticker back", async () => {
    const renderer = await PixiRenderer.create(document.createElement("div"), SIZE);
    renderer.draw(frameWith(["main", "a1"]));
    const app = pixi.state.apps[0] as InstanceType<typeof pixi.Application>;
    const parts = all(app).filter((c) => c !== app.stage && !app.stage.children.includes(c));
    expect(parts.length).toBeGreaterThan(0);

    renderer.destroy();
    expect(app.destroy).toHaveBeenCalledWith({ removeView: true }, { children: true });
    // Every figure, name and bubble was destroyed (bubbles with their children).
    // Desks too, not only characters.
    for (const part of parts) {
      const destroyedHere = part.destroyed;
      const insideDestroyedBubble = app.stage.children.some((layer) =>
        layer.children.some((child) => child.destroyed && child.children.includes(part)),
      );
      expect(destroyedHere || insideDestroyedBubble).toBe(true);
    }
    expect(pixi.Ticker.system.autoStart).toBe(true);

    // A second destroy does nothing.
    renderer.destroy();
    expect(app.destroy).toHaveBeenCalledTimes(1);
  });

  it("keeps the ticker held until the last renderer goes", async () => {
    const a = await PixiRenderer.create(document.createElement("div"), SIZE);
    const b = await PixiRenderer.create(document.createElement("div"), SIZE);
    a.destroy();
    expect(pixi.Ticker.system.autoStart).toBe(false);
    b.destroy();
    expect(pixi.Ticker.system.autoStart).toBe(true);
  });

  it("gives the ticker back when the canvas cannot be created", async () => {
    pixi.state.failInit = true;
    await expect(PixiRenderer.create(document.createElement("div"), SIZE)).rejects.toThrow("no WebGL");
    expect(pixi.Ticker.system.autoStart).toBe(true);
  });

  it("destroys the sprite of a character that is gone", async () => {
    const renderer = await PixiRenderer.create(document.createElement("div"), SIZE);
    renderer.draw(frameWith(["main", "a1"]));
    const app = pixi.state.apps[0] as InstanceType<typeof pixi.Application>;
    const [, people, labels, bubbles] = app.stage.children as InstanceType<typeof pixi.Container>[];
    const label = labels?.children.find((l) => (l as InstanceType<typeof pixi.BitmapText>).text === "a1");
    expect(label).toBeDefined();
    renderer.draw(frameWith(["main"]));
    expect(label?.destroyed).toBe(true);
    expect(people && bubbles).toBeTruthy();
    renderer.destroy();
  });
});
