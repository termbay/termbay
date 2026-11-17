import { describe, expect, it, vi } from "vitest";
import { Emitter } from "../transport/emitter";
import { RendererController, type WebglAddonLike } from "./renderer";

class FakeWebgl implements WebglAddonLike {
  readonly contextLoss = new Emitter<void>();
  disposed = false;
  onContextLoss(l: () => void) {
    return this.contextLoss.on(l);
  }
  dispose() {
    this.disposed = true;
  }
}

describe("RendererController", () => {
  it("uses WebGL when it loads", () => {
    const addon = new FakeWebgl();
    const host = { loadAddon: vi.fn() };
    const controller = new RendererController(host, () => addon);
    expect(controller.start()).toBe("webgl");
    expect(host.loadAddon).toHaveBeenCalledWith(addon);
  });

  it("falls back to DOM when WebGL fails to load", () => {
    const onChange = vi.fn();
    const host = {
      loadAddon: () => {
        throw new Error("no webgl2");
      },
    };
    const controller = new RendererController(host, () => new FakeWebgl(), onChange);
    expect(controller.start()).toBe("dom");
    expect(onChange).toHaveBeenCalledWith("dom", expect.any(Error));
  });

  it("switches to DOM and disposes the addon on context loss", () => {
    const addon = new FakeWebgl();
    const onChange = vi.fn();
    const controller = new RendererController({ loadAddon: () => {} }, () => addon, onChange);
    controller.start();

    addon.contextLoss.emit();

    expect(controller.kind).toBe("dom");
    expect(addon.disposed).toBe(true);
    expect(onChange).toHaveBeenLastCalledWith("dom", "webgl context lost");
  });
});

describe("RendererController.stop (hidden tab)", () => {
  it("disposes the addon and reloads a new one on start", () => {
    const made: FakeWebgl[] = [];
    const host = { loadAddon: vi.fn() };
    const controller = new RendererController(host, () => {
      const a = new FakeWebgl();
      made.push(a);
      return a;
    });
    controller.start();
    controller.stop();
    expect(made[0]?.disposed).toBe(true);
    expect(controller.kind).toBe("dom");
    expect(controller.start()).toBe("webgl");
    expect(made).toHaveLength(2);
    controller.start(); // already active: no third addon
    expect(made).toHaveLength(2);
  });

  it("does not retry WebGL after it failed", () => {
    const addon = new FakeWebgl();
    const create = vi.fn(() => addon);
    const controller = new RendererController({ loadAddon: () => {} }, create);
    controller.start();
    addon.contextLoss.emit();
    controller.stop();
    expect(controller.start()).toBe("dom");
    expect(create).toHaveBeenCalledOnce();
  });
});
