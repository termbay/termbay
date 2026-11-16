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
