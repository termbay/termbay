// @vitest-environment happy-dom
import { describe, expect, it, vi } from "vitest";

import type { Size } from "./layout";
import { browserHost, realSize, type HostWindow } from "./scene";

/** A container whose size the test sets. */
function panel(width: number, height: number) {
  const el = document.createElement("div");
  const size = { width, height };
  Object.defineProperty(el, "clientWidth", { get: () => size.width });
  Object.defineProperty(el, "clientHeight", { get: () => size.height });
  return { el, size };
}

class FakeObserver {
  static last: FakeObserver | null = null;
  observed: Element[] = [];
  disconnected = false;
  constructor(readonly callback: () => void) {
    FakeObserver.last = this;
  }
  observe(el: Element) {
    this.observed.push(el);
  }
  unobserve() {}
  disconnect() {
    this.disconnected = true;
  }
}

function fakeWindow(withObserver: boolean) {
  const winTarget = new EventTarget();
  const docTarget = new EventTarget();
  const doc = {
    hidden: false,
    addEventListener: docTarget.addEventListener.bind(docTarget),
    removeEventListener: docTarget.removeEventListener.bind(docTarget),
  };
  const win = {
    document: doc,
    requestAnimationFrame: vi.fn(() => 7),
    cancelAnimationFrame: vi.fn(),
    setTimeout: vi.fn(() => 9),
    clearTimeout: vi.fn(),
    addEventListener: winTarget.addEventListener.bind(winTarget),
    removeEventListener: winTarget.removeEventListener.bind(winTarget),
    ...(withObserver ? { ResizeObserver: FakeObserver as unknown as typeof ResizeObserver } : {}),
  };
  return {
    win: win as unknown as HostWindow,
    raw: win,
    doc,
    resize: () => winTarget.dispatchEvent(new Event("resize")),
    visibility: (hidden: boolean) => {
      doc.hidden = hidden;
      docTarget.dispatchEvent(new Event("visibilitychange"));
    },
  };
}

describe("browserHost", () => {
  it("measures the panel and uses the window's frames and timers", () => {
    const { el } = panel(320, 600);
    const { win, raw } = fakeWindow(true);
    const host = browserHost(el, win);
    expect(host.size).toEqual({ width: 320, height: 600 });
    const cb = () => {};
    expect(host.clock.request(cb)).toBe(7);
    host.clock.cancel(7);
    expect(raw.cancelAnimationFrame).toHaveBeenCalledWith(7);
    expect(host.clock.setTimer(cb, 400)).toBe(9);
    expect(raw.setTimeout).toHaveBeenCalledWith(cb, 400);
    host.clock.clearTimer(9);
    expect(raw.clearTimeout).toHaveBeenCalledWith(9);
  });

  it("reports sizes from a ResizeObserver on the panel, never 0 x 0, and disconnects", () => {
    const { el, size } = panel(320, 600);
    const { win } = fakeWindow(true);
    const sizes: Size[] = [];
    const stop = browserHost(el, win).observeSize?.((s) => sizes.push(s));
    const observer = FakeObserver.last as FakeObserver;
    expect(observer.observed).toEqual([el]);
    size.width = 640;
    observer.callback();
    // The panel is hidden: 0 x 0 is not reported.
    size.width = 0;
    size.height = 0;
    observer.callback();
    size.width = 200;
    observer.callback();
    expect(sizes).toEqual([{ width: 640, height: 600 }]);
    stop?.();
    expect(observer.disconnected).toBe(true);
  });

  it("falls back to the window's resize event without ResizeObserver", () => {
    const { el, size } = panel(320, 600);
    const { win, resize } = fakeWindow(false);
    const sizes: Size[] = [];
    const stop = browserHost(el, win).observeSize?.((s) => sizes.push(s));
    size.height = 400;
    resize();
    size.height = 0;
    resize();
    expect(sizes).toEqual([{ width: 320, height: 400 }]);
    stop?.();
    size.height = 500;
    resize();
    expect(sizes).toHaveLength(1);
  });

  it("follows visibilitychange, and stops when asked", () => {
    const { el } = panel(320, 600);
    const { win, visibility, doc } = fakeWindow(true);
    doc.hidden = true;
    const host = browserHost(el, win);
    expect(host.pageHidden).toBe(true);
    const seen: boolean[] = [];
    const stop = host.observePage?.((hidden) => seen.push(hidden));
    visibility(false);
    visibility(true);
    stop?.();
    visibility(false);
    expect(seen).toEqual([false, true]);
  });

  it("realSize rejects empty sizes", () => {
    expect(realSize({ width: 1, height: 1 })).toBe(true);
    expect(realSize({ width: 0, height: 10 })).toBe(false);
    expect(realSize({ width: 10, height: 0 })).toBe(false);
  });
});
