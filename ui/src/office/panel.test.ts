// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";

import type { OfficeEvent, OfficeListener, OfficeSnapshot } from "../transport/office";
import type { OfficeContent } from "./content";
import {
  KEY_STEP,
  MAX_HELD_EVENTS,
  OfficePanel,
  RESUBSCRIBE_RETRY_MS,
  WIDTH_KEY,
  localWidthStore,
  type WidthStore,
} from "./panel";
import { DEFAULT_PANEL_WIDTH, MAX_PANEL_WIDTH, MIN_PANEL_WIDTH, MIN_TERMINAL_WIDTH, PanelModel } from "./panel-model";

const event = (fields: Partial<OfficeEvent> = {}): OfficeEvent => ({ v: 1, session: "s1", agent: "main", kind: "tool", ...fields });
const empty: OfficeSnapshot = { v: 1, sessions: [] };
const busy: OfficeSnapshot = { v: 1, sessions: [{ session: "s1", agents: [{ id: "main", activity: "working" }], tasks: [] }] };

class FakeContent implements OfficeContent {
  readonly calls: string[] = [];
  snapshot(s: OfficeSnapshot) {
    this.calls.push(`snapshot:${s.sessions.length}`);
  }
  event(e: OfficeEvent) {
    this.calls.push(`event:${e.target ?? e.kind}`);
  }
  setVisible(v: boolean) {
    this.calls.push(`visible:${v}`);
  }
  destroy() {
    this.calls.push("destroy");
  }
}

function memoryStore(initial?: number): WidthStore & { saved: number[] } {
  const saved: number[] = [];
  return { saved, load: () => initial, save: (w) => void saved.push(w) };
}

interface Setup {
  autoOpen?: boolean;
  width?: number;
  store?: WidthStore;
  resubscribe?: (listener: OfficeListener) => Promise<void>;
  load?: () => Promise<OfficeContent>;
}

let current: OfficePanel | null = null;
afterEach(() => {
  current?.destroy();
  current = null;
  document.body.replaceChildren();
});

function setup(options: Setup = {}) {
  const root = document.createElement("div");
  const terminals = document.createElement("main");
  const panel = document.createElement("aside");
  panel.hidden = true;
  root.append(terminals, panel);
  document.body.append(root);
  const content = new FakeContent();
  const load = vi.fn(options.load ?? (async () => content));
  const focusTerminal = vi.fn();
  const office = new OfficePanel({
    root,
    panel,
    model: new PanelModel({ autoOpen: options.autoOpen ?? true, width: options.width }),
    load,
    store: options.store,
    resubscribe: options.resubscribe,
    focusTerminal,
    viewport: () => 1600,
  });
  current = office;
  return { root, panel, office, content, load, focusTerminal };
}

describe("OfficePanel", () => {
  it("stays closed and loads nothing while the office is empty", () => {
    const { panel, office, load } = setup();
    office.snapshot(empty);
    expect(panel.hidden).toBe(true);
    expect(office.dividerElement.hidden).toBe(true);
    expect(load).not.toHaveBeenCalled();
  });

  it("opens on the first agent event and hands the content what it missed", async () => {
    const { panel, office, content, load } = setup();
    office.snapshot(empty);
    office.event(event({ target: "a.ts" }));
    expect(panel.hidden).toBe(false);
    expect(office.dividerElement.hidden).toBe(false);
    expect(panel.style.width).toBe(`${DEFAULT_PANEL_WIDTH}px`);
    expect(load).toHaveBeenCalledTimes(1);
    expect(load).toHaveBeenCalledWith(office.contentHost);

    office.event(event({ target: "b.ts" })); // while loading
    await office.ready();
    office.event(event({ target: "c.ts" })); // live
    expect(content.calls).toEqual(["snapshot:0", "event:a.ts", "event:b.ts", "visible:true", "event:c.ts"]);
  });

  it("opens when the first picture already has agents", async () => {
    const { panel, office } = setup();
    office.snapshot(busy);
    expect(panel.hidden).toBe(false);
    await office.ready();
  });

  it("closes only by the shortcut or the button, and hides the content", async () => {
    const { panel, office, content, load, focusTerminal } = setup();
    office.snapshot(empty);
    office.event(event());
    await office.ready();

    office.toggle();
    expect(panel.hidden).toBe(true);
    expect(office.dividerElement.hidden).toBe(true);
    expect(content.calls.at(-1)).toBe("visible:false");

    // More agents do not reopen it.
    office.event(event({ target: "later" }));
    expect(panel.hidden).toBe(true);
    expect(content.calls.at(-1)).toBe("event:later");

    office.toggle();
    expect(panel.hidden).toBe(false);
    expect(content.calls.at(-1)).toBe("visible:true");
    expect(load).toHaveBeenCalledTimes(1);

    panel.querySelector<HTMLButtonElement>(".office-close")?.click();
    expect(panel.hidden).toBe(true);
    expect(content.calls.at(-1)).toBe("visible:false");
    expect(focusTerminal).toHaveBeenCalledTimes(1);
  });

  it("closed by the person before any agent, it does not open by itself", () => {
    const { panel, office, load } = setup();
    office.toggle();
    office.toggle();
    office.event(event());
    expect(panel.hidden).toBe(true);
    expect(load).toHaveBeenCalledTimes(1);
  });

  it("with auto_open off, opens only by the shortcut", async () => {
    const { panel, office, content, load } = setup({ autoOpen: false });
    office.snapshot(busy);
    office.event(event());
    expect(panel.hidden).toBe(true);
    expect(load).not.toHaveBeenCalled();

    office.toggle();
    await office.ready();
    expect(panel.hidden).toBe(false);
    expect(content.calls).toEqual(["snapshot:1", "event:tool", "visible:true"]);
  });

  it("asks for a fresh subscription when it missed too many changes", async () => {
    const resubscribe = vi.fn(async (listener: OfficeListener) => listener.snapshot(busy));
    const { office, content } = setup({ autoOpen: false, resubscribe });
    office.snapshot(empty);
    for (let i = 0; i <= MAX_HELD_EVENTS; i++) office.event(event({ target: `e${i}` }));
    office.toggle();
    await office.ready();
    // The panel stays the only listener: the new subscription replaces the old one.
    expect(resubscribe).toHaveBeenCalledWith(office);
    office.event(event({ target: "live" }));
    expect(content.calls).toEqual(["snapshot:1", "visible:true", "event:live"]);
  });

  it("tries the fresh subscription again until it works, showing no old state meanwhile", async () => {
    vi.useFakeTimers();
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    try {
      let failures = 2;
      const resubscribe = vi.fn(async (listener: OfficeListener) => {
        if (failures-- > 0) throw new Error("ipc down");
        listener.snapshot(busy);
      });
      const { office, content } = setup({ autoOpen: false, resubscribe });
      office.snapshot(empty);
      for (let i = 0; i <= MAX_HELD_EVENTS; i++) office.event(event({ target: `e${i}` }));
      office.toggle();
      await office.ready();
      await vi.advanceTimersByTimeAsync(0);
      expect(resubscribe).toHaveBeenCalledTimes(1);
      // Changes after an old picture would be wrong: dropped until the fresh one.
      office.event(event({ target: "while-down" }));
      expect(content.calls).toEqual(["visible:true"]);

      await vi.advanceTimersByTimeAsync(RESUBSCRIBE_RETRY_MS);
      expect(resubscribe).toHaveBeenCalledTimes(2);
      await vi.advanceTimersByTimeAsync(RESUBSCRIBE_RETRY_MS * 2);
      expect(resubscribe).toHaveBeenCalledTimes(3);
      office.event(event({ target: "live" }));
      expect(content.calls).toEqual(["visible:true", "snapshot:1", "event:live"]);
      expect(error).toHaveBeenCalledTimes(2);
    } finally {
      error.mockRestore();
      vi.useRealTimers();
    }
  });

  it("stops trying when destroyed", async () => {
    vi.useFakeTimers();
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    try {
      const resubscribe = vi.fn(async () => Promise.reject(new Error("ipc down")));
      const { office } = setup({ autoOpen: false, resubscribe });
      office.snapshot(empty);
      for (let i = 0; i <= MAX_HELD_EVENTS; i++) office.event(event());
      office.toggle();
      await office.ready();
      await vi.advanceTimersByTimeAsync(0);
      office.destroy();
      current = null;
      await vi.advanceTimersByTimeAsync(60_000);
      expect(resubscribe).toHaveBeenCalledTimes(1);
    } finally {
      error.mockRestore();
      vi.useRealTimers();
    }
  });

  it("without a way to resubscribe, keeps every change instead of an old picture", async () => {
    const { office, content } = setup({ autoOpen: false });
    office.snapshot(busy);
    for (let i = 0; i < MAX_HELD_EVENTS + 10; i++) office.event(event({ target: `e${i}` }));
    office.toggle();
    await office.ready();
    expect(content.calls[0]).toBe("snapshot:1");
    expect(content.calls.filter((c) => c.startsWith("event:")).length).toBe(MAX_HELD_EVENTS + 10);
    expect(content.calls.at(-2)).toBe(`event:e${MAX_HELD_EVENTS + 9}`);
  });

  it("tries again on the next opening when the content fails to load", async () => {
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    const content = new FakeContent();
    let fail = true;
    const { office } = setup({ autoOpen: false, load: async () => (fail ? Promise.reject(new Error("no")) : content) });
    office.toggle();
    await office.ready();
    expect(error).toHaveBeenCalled();
    fail = false;
    office.toggle();
    office.toggle();
    await office.ready();
    expect(content.calls).toEqual(["visible:true"]);
    error.mockRestore();
  });

  it("drags the divider within the limits and remembers the width", () => {
    const store = memoryStore();
    const { root, panel, office } = setup({ autoOpen: false, store });
    vi.spyOn(root, "getBoundingClientRect").mockReturnValue({ right: 1600 } as DOMRect);
    office.toggle();
    const divider = office.dividerElement;
    const pointer = (type: string, clientX: number) =>
      divider.dispatchEvent(new PointerEvent(type, { clientX, button: 0, pointerId: 1, bubbles: true }));

    pointer("pointerdown", 1240);
    expect(root.classList.contains("office-resizing")).toBe(true);
    pointer("pointermove", 1100);
    expect(panel.style.width).toBe("500px");
    pointer("pointermove", 1590);
    expect(panel.style.width).toBe(`${MIN_PANEL_WIDTH}px`);
    expect(store.saved).toEqual([]);
    pointer("pointerup", 1590);
    expect(root.classList.contains("office-resizing")).toBe(false);
    expect(store.saved).toEqual([MIN_PANEL_WIDTH]);

    // Moving after the drag ended changes nothing.
    pointer("pointermove", 1000);
    expect(panel.style.width).toBe(`${MIN_PANEL_WIDTH}px`);
  });

  it("ends the drag when the pointer capture is lost", () => {
    const store = memoryStore();
    const { root, panel, office } = setup({ autoOpen: false, store });
    vi.spyOn(root, "getBoundingClientRect").mockReturnValue({ right: 1600 } as DOMRect);
    office.toggle();
    const divider = office.dividerElement;
    divider.dispatchEvent(new PointerEvent("pointerdown", { clientX: 1240, button: 0, pointerId: 1 }));
    divider.dispatchEvent(new PointerEvent("pointermove", { clientX: 1100, pointerId: 1 }));
    divider.dispatchEvent(new PointerEvent("lostpointercapture", { pointerId: 1 }));
    expect(root.classList.contains("office-resizing")).toBe(false);
    expect(store.saved).toEqual([500]);
    divider.dispatchEvent(new PointerEvent("pointermove", { clientX: 900, pointerId: 1 }));
    expect(panel.style.width).toBe("500px");
  });

  it("destroy during a drag removes its listeners and the resizing class", () => {
    const store = memoryStore();
    const { root, panel, office } = setup({ autoOpen: false, store });
    vi.spyOn(root, "getBoundingClientRect").mockReturnValue({ right: 1600 } as DOMRect);
    office.toggle();
    const divider = office.dividerElement;
    divider.dispatchEvent(new PointerEvent("pointerdown", { clientX: 1240, button: 0, pointerId: 1 }));
    divider.dispatchEvent(new PointerEvent("pointermove", { clientX: 1100, pointerId: 1 }));
    office.destroy();
    current = null;
    expect(root.classList.contains("office-resizing")).toBe(false);
    divider.dispatchEvent(new PointerEvent("pointermove", { clientX: 900, pointerId: 1 }));
    expect(panel.style.width).toBe("500px");
    expect(store.saved).toEqual([]);
  });

  it("tells assistive technology the divider's limits", () => {
    const { office } = setup({ autoOpen: false });
    office.toggle();
    const divider = office.dividerElement;
    expect(divider.getAttribute("aria-valuemin")).toBe(String(MIN_PANEL_WIDTH));
    expect(divider.getAttribute("aria-valuemax")).toBe(String(Math.min(MAX_PANEL_WIDTH, 1600 - MIN_TERMINAL_WIDTH)));
    expect(divider.getAttribute("aria-valuenow")).toBe(String(DEFAULT_PANEL_WIDTH));
  });

  it("resizes with the arrow keys and resets on double click", () => {
    const store = memoryStore();
    const { panel, office } = setup({ autoOpen: false, store, width: 400 });
    office.toggle();
    const divider = office.dividerElement;
    expect(panel.style.width).toBe("400px");
    divider.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowLeft" }));
    expect(panel.style.width).toBe(`${400 + KEY_STEP}px`);
    divider.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight" }));
    divider.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight" }));
    expect(panel.style.width).toBe(`${400 - KEY_STEP}px`);
    expect(divider.getAttribute("aria-valuenow")).toBe(String(400 - KEY_STEP));
    divider.dispatchEvent(new MouseEvent("dblclick"));
    expect(panel.style.width).toBe(`${DEFAULT_PANEL_WIDTH}px`);
    expect(store.saved.at(-1)).toBe(DEFAULT_PANEL_WIDTH);
  });

  it("destroys the content and removes the divider", async () => {
    const { root, office, content } = setup();
    office.event(event());
    await office.ready();
    office.destroy();
    current = null;
    expect(content.calls.at(-1)).toBe("destroy");
    expect(root.querySelector(".office-divider")).toBeNull();
  });
});

describe("localWidthStore", () => {
  it("reads and writes the width, ignoring bad values", () => {
    const storage = window.localStorage;
    storage.removeItem(WIDTH_KEY);
    const store = localWidthStore(() => storage);
    expect(store.load()).toBeUndefined();
    store.save(432.4);
    expect(storage.getItem(WIDTH_KEY)).toBe("432");
    expect(store.load()).toBe(432);
    storage.setItem(WIDTH_KEY, "wide");
    expect(store.load()).toBeUndefined();
    storage.removeItem(WIDTH_KEY);
  });

  it("never throws when the storage is missing or refuses access", () => {
    const refusing = () => {
      throw new Error("SecurityError");
    };
    expect(localWidthStore(refusing).load()).toBeUndefined();
    expect(() => localWidthStore(refusing).save(300)).not.toThrow();
    expect(localWidthStore(() => null).load()).toBeUndefined();
  });
});
