import { describe, expect, it, vi } from "vitest";
import type { Os } from "../platform/os";
import { installShortcuts, isBrowserKey, resolveShortcut, type ShortcutKeyEvent } from "./shortcuts";

interface Mods {
  ctrl?: boolean;
  shift?: boolean;
  alt?: boolean;
  meta?: boolean;
  repeat?: boolean;
  isComposing?: boolean;
  keyCode?: number;
  code?: string;
}

function key(k: string, m: Mods = {}): ShortcutKeyEvent {
  const code = m.code ?? (/^[a-z]$/i.test(k) ? `Key${k.toUpperCase()}` : /^[0-9]$/.test(k) ? `Digit${k}` : k);
  return {
    type: "keydown",
    key: k,
    code,
    ctrlKey: m.ctrl ?? false,
    shiftKey: m.shift ?? false,
    altKey: m.alt ?? false,
    metaKey: m.meta ?? false,
    repeat: m.repeat ?? false,
    isComposing: m.isComposing ?? false,
    keyCode: m.keyCode ?? 0,
  };
}

const cmd = { meta: true };
const cs = { ctrl: true, shift: true };

const MAC: Array<[ShortcutKeyEvent, unknown]> = [
  [key("t", cmd), { type: "newTab" }],
  [key("w", cmd), { type: "closePane" }],
  [key("d", cmd), { type: "split", direction: "right" }],
  [key("D", { ...cmd, shift: true }), { type: "split", direction: "down" }],
  [key("ArrowLeft", { ...cmd, alt: true }), { type: "focus", direction: "left" }],
  [key("ArrowRight", { ...cmd, alt: true }), { type: "focus", direction: "right" }],
  [key("ArrowUp", { ...cmd, alt: true }), { type: "focus", direction: "up" }],
  [key("ArrowDown", { ...cmd, alt: true }), { type: "focus", direction: "down" }],
  [key("}", { ...cmd, shift: true, code: "BracketRight" }), { type: "nextTab" }],
  [key("{", { ...cmd, shift: true, code: "BracketLeft" }), { type: "prevTab" }],
  [key("Tab", { ctrl: true }), { type: "nextTab" }],
  [key("Tab", { ctrl: true, shift: true }), { type: "prevTab" }],
  [key("1", cmd), { type: "goToTab", index: 1 }],
  [key("8", cmd), { type: "goToTab", index: 8 }],
  [key("9", cmd), { type: "goToTab", index: 9 }],
  [key("f", cmd), { type: "find" }],
  [key("O", { ...cmd, shift: true }), { type: "toggleOffice" }],
  [key("B", { ...cmd, shift: true }), { type: "toggleTabs" }],
  // Non-Latin layout: letter by physical key.
  [key("е", { ...cmd, code: "KeyT" }), { type: "newTab" }],
];

const PC: Array<[ShortcutKeyEvent, unknown]> = [
  [key("T", cs), { type: "newTab" }],
  [key("W", cs), { type: "closePane" }],
  [key("D", cs), { type: "split", direction: "right" }],
  [key("S", cs), { type: "split", direction: "down" }],
  [key("ArrowLeft", cs), { type: "focus", direction: "left" }],
  [key("ArrowRight", cs), { type: "focus", direction: "right" }],
  [key("ArrowUp", cs), { type: "focus", direction: "up" }],
  [key("ArrowDown", cs), { type: "focus", direction: "down" }],
  [key("Tab", { ctrl: true }), { type: "nextTab" }],
  [key("Tab", cs), { type: "prevTab" }],
  // Shift changes the character ("!"), the code stays Digit1.
  [key("!", { ...cs, code: "Digit1" }), { type: "goToTab", index: 1 }],
  [key("(", { ...cs, code: "Digit9" }), { type: "goToTab", index: 9 }],
  [key("F", cs), { type: "find" }],
  [key("O", cs), { type: "toggleOffice" }],
  [key("B", cs), { type: "toggleTabs" }],
];

describe("resolveShortcut: the whole table", () => {
  it.each(MAC)("macOS %#", (ev, action) => expect(resolveShortcut(ev, "macos")).toEqual(action));
  for (const os of ["linux", "windows"] as const) {
    it.each(PC)(`${os} %#`, (ev, action) => expect(resolveShortcut(ev, os)).toEqual(action));
  }
});

describe("resolveShortcut: shell keys are never taken", () => {
  const shellKeys: Array<[string, ShortcutKeyEvent]> = [
    ["Ctrl+C", key("c", { ctrl: true })],
    ["Ctrl+D", key("d", { ctrl: true })],
    ["Ctrl+R", key("r", { ctrl: true })],
    ["Ctrl+W", key("w", { ctrl: true })],
    ["Ctrl+T", key("t", { ctrl: true })],
    ["Ctrl+B (tmux prefix, readline back)", key("b", { ctrl: true })],
    ["Cmd+B alone is not toggleTabs", key("b", { meta: true })],
    ["Alt+B", key("b", { alt: true })],
    ["AltGr+Q (Ctrl+Alt)", key("/", { ctrl: true, alt: true, code: "KeyQ" })],
    ["Ctrl+Alt+Shift+T", key("T", { ctrl: true, alt: true, shift: true })],
    ["Tab", key("Tab")],
    ["Shift+Tab", key("Tab", { shift: true })],
    ["Ctrl+Space", key(" ", { ctrl: true, code: "Space" })],
    ["Ctrl+1", key("1", { ctrl: true })],
    ["Shift+ArrowLeft", key("ArrowLeft", { shift: true })],
  ];
  const all: Os[] = ["macos", "linux", "windows"];
  for (const [name, ev] of shellKeys) {
    it(`${name} reaches the terminal on every OS`, () => {
      for (const os of all) expect(resolveShortcut(ev, os)).toBeNull();
    });
  }

  it("Option+letter on macOS reaches the terminal", () => {
    expect(resolveShortcut(key("†", { alt: true, code: "KeyT" }), "macos")).toBeNull();
    expect(resolveShortcut(key("∂", { alt: true, code: "KeyD" }), "macos")).toBeNull();
  });

  it("Cmd shortcuts do nothing on Linux/Windows and Ctrl+Shift does nothing on macOS", () => {
    expect(resolveShortcut(key("t", cmd), "linux")).toBeNull();
    expect(resolveShortcut(key("T", cs), "macos")).toBeNull();
    expect(resolveShortcut(key("w", { meta: true, ctrl: true }), "macos")).toBeNull();
  });

  it("Ctrl+Shift+C/V stay with copy and paste", () => {
    expect(resolveShortcut(key("C", cs), "linux")).toBeNull();
    expect(resolveShortcut(key("V", cs), "windows")).toBeNull();
    expect(resolveShortcut(key("c", cmd), "macos")).toBeNull();
  });

  it("ignores IME composition, dead keys and keyup", () => {
    expect(resolveShortcut(key("t", { ...cmd, isComposing: true }), "macos")).toBeNull();
    expect(resolveShortcut(key("T", { ...cs, keyCode: 229 }), "linux")).toBeNull();
    expect(resolveShortcut({ ...key("t", cmd), type: "keyup" }, "macos")).toBeNull();
  });

  it("ignores auto-repeat for create/close, not for navigation", () => {
    expect(resolveShortcut(key("t", { ...cmd, repeat: true }), "macos")).toBeNull();
    expect(resolveShortcut(key("W", { ...cs, repeat: true }), "linux")).toBeNull();
    expect(resolveShortcut(key("d", { ...cmd, repeat: true }), "macos")).toBeNull();
    expect(resolveShortcut(key("Tab", { ctrl: true, repeat: true }), "linux")).toEqual({ type: "nextTab" });
  });
});

describe("isBrowserKey", () => {
  it("covers reload, print, devtools and find", () => {
    for (const ev of [key("F5"), key("r", { ctrl: true }), key("R", cs), key("p", { ctrl: true }), key("I", cs), key("f", { ctrl: true })]) {
      expect(isBrowserKey(ev)).toBe(true);
    }
    expect(isBrowserKey(key("a", { ctrl: true }))).toBe(false);
    expect(isBrowserKey(key("r"))).toBe(false);
  });
});

describe("installShortcuts", () => {
  class FakeWindow {
    listener: ((ev: KeyboardEvent) => void) | null = null;
    capture = false;
    addEventListener(_: string, l: (ev: KeyboardEvent) => void, o: { capture: boolean }) {
      this.listener = l;
      this.capture = o.capture;
    }
    removeEventListener() {
      this.listener = null;
    }
    fire(ev: ShortcutKeyEvent) {
      const e = { ...ev, preventDefault: vi.fn(), stopPropagation: vi.fn() };
      this.listener?.(e as unknown as KeyboardEvent);
      return e;
    }
  }

  it("listens in capture, consumes app shortcuts and lets the rest through", () => {
    const win = new FakeWindow();
    const run = vi.fn();
    const sub = installShortcuts(win, "linux", run);
    expect(win.capture).toBe(true);
    const taken = win.fire(key("T", cs));
    expect(run).toHaveBeenCalledWith({ type: "newTab" });
    expect(taken.stopPropagation).toHaveBeenCalled();
    const shell = win.fire(key("c", { ctrl: true }));
    expect(shell.preventDefault).not.toHaveBeenCalled();
    expect(shell.stopPropagation).not.toHaveBeenCalled();
    // A held create key is swallowed without acting.
    run.mockClear();
    const held = win.fire(key("T", { ...cs, repeat: true }));
    expect(run).not.toHaveBeenCalled();
    expect(held.stopPropagation).toHaveBeenCalled();
    sub.dispose();
    expect(win.listener).toBeNull();
  });

  it("on Windows, prevents only the browser default of Ctrl+R, which still reaches xterm.js", () => {
    const win = new FakeWindow();
    installShortcuts(win, "windows", vi.fn());
    const e = win.fire(key("r", { ctrl: true }));
    expect(e.preventDefault).toHaveBeenCalled();
    expect(e.stopPropagation).not.toHaveBeenCalled();
  });
});
