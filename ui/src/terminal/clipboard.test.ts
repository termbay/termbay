import { describe, expect, it, vi } from "vitest";
import type { Os } from "../platform/os";
import { clipboardActionFor, createClipboardKeyHandler, type ClipboardTerminal, type KeyEventLike } from "./clipboard";

function key(
  k: string,
  mods: Partial<Pick<KeyEventLike, "ctrlKey" | "shiftKey" | "altKey" | "metaKey">> = {},
  type = "keydown",
  code = /^[a-z]$/i.test(k) ? `Key${k.toUpperCase()}` : "",
): KeyEventLike & { preventDefault: ReturnType<typeof vi.fn<() => void>> } {
  return {
    type,
    key: k,
    code,
    ctrlKey: false,
    shiftKey: false,
    altKey: false,
    metaKey: false,
    ...mods,
    preventDefault: vi.fn<() => void>(),
  };
}

const tick = () => new Promise((r) => setTimeout(r, 0));

describe("clipboardActionFor", () => {
  it("uses Cmd+C / Cmd+V on macOS and leaves Ctrl+C to the shell", () => {
    expect(clipboardActionFor(key("c", { metaKey: true }), "macos")).toBe("copy");
    expect(clipboardActionFor(key("v", { metaKey: true }), "macos")).toBe("paste");
    expect(clipboardActionFor(key("c", { ctrlKey: true }), "macos")).toBeNull();
    expect(clipboardActionFor(key("C", { ctrlKey: true, shiftKey: true }), "macos")).toBeNull();
  });

  it.each<Os>(["linux", "windows"])("uses Ctrl+Shift+C / Ctrl+Shift+V on %s and leaves Ctrl+C/Ctrl+V to the shell", (os) => {
    expect(clipboardActionFor(key("C", { ctrlKey: true, shiftKey: true }), os)).toBe("copy");
    expect(clipboardActionFor(key("V", { ctrlKey: true, shiftKey: true }), os)).toBe("paste");
    expect(clipboardActionFor(key("c", { ctrlKey: true }), os)).toBeNull();
    expect(clipboardActionFor(key("v", { ctrlKey: true }), os)).toBeNull();
    expect(clipboardActionFor(key("C", { ctrlKey: true, shiftKey: true, altKey: true }), os)).toBeNull();
    expect(clipboardActionFor(key("c", { metaKey: true }), os)).toBeNull();
  });

  it("falls back to the physical key on non-Latin layouts", () => {
    expect(clipboardActionFor(key("С", { ctrlKey: true, shiftKey: true }, "keydown", "KeyC"), "linux")).toBe("copy");
  });
});

function fakeTerm(selection = ""): ClipboardTerminal & { pasted: string[] } {
  const pasted: string[] = [];
  return { pasted, hasSelection: () => selection.length > 0, getSelection: () => selection, paste: (t) => pasted.push(t) };
}

describe("createClipboardKeyHandler", () => {
  it("lets every other key reach xterm.js", () => {
    const handler = createClipboardKeyHandler(fakeTerm(), "linux", undefined);
    const ctrlC = key("c", { ctrlKey: true });
    expect(handler(ctrlC)).toBe(true);
    expect(ctrlC.preventDefault).not.toHaveBeenCalled();
    expect(handler(key("a"))).toBe(true);
  });

  it("copies the selection with Ctrl+Shift+C on linux", async () => {
    const clipboard = { writeText: vi.fn(async () => {}), readText: vi.fn(async () => "") };
    const handler = createClipboardKeyHandler(fakeTerm("hello"), "linux", clipboard);
    const ev = key("C", { ctrlKey: true, shiftKey: true });
    expect(handler(ev)).toBe(false);
    expect(ev.preventDefault).toHaveBeenCalled();
    expect(clipboard.writeText).toHaveBeenCalledWith("hello");
  });

  it("does not touch the clipboard when nothing is selected", () => {
    const clipboard = { writeText: vi.fn(async () => {}), readText: vi.fn(async () => "") };
    createClipboardKeyHandler(fakeTerm(), "windows", clipboard)(key("C", { ctrlKey: true, shiftKey: true }));
    expect(clipboard.writeText).not.toHaveBeenCalled();
  });

  it("pastes through term.paste (bracketed paste) with Ctrl+Shift+V on windows", async () => {
    const term = fakeTerm();
    const clipboard = { writeText: vi.fn(async () => {}), readText: vi.fn(async () => "ls -la\n") };
    const handler = createClipboardKeyHandler(term, "windows", clipboard);
    expect(handler(key("V", { ctrlKey: true, shiftKey: true }))).toBe(false);
    await tick();
    expect(term.pasted).toEqual(["ls -la\n"]);
  });

  it("swallows keyup of the shortcut without acting twice", () => {
    const clipboard = { writeText: vi.fn(async () => {}), readText: vi.fn(async () => "") };
    const handler = createClipboardKeyHandler(fakeTerm("x"), "linux", clipboard);
    expect(handler(key("C", { ctrlKey: true, shiftKey: true }, "keyup"))).toBe(false);
    expect(clipboard.writeText).not.toHaveBeenCalled();
  });

  it("keeps the native copy/paste path on macOS", () => {
    const clipboard = { writeText: vi.fn(async () => {}), readText: vi.fn(async () => "") };
    const handler = createClipboardKeyHandler(fakeTerm("x"), "macos", clipboard);
    const ev = key("v", { metaKey: true });
    expect(handler(ev)).toBe(false);
    expect(ev.preventDefault).not.toHaveBeenCalled();
    expect(clipboard.readText).not.toHaveBeenCalled();
  });

  it("reports clipboard failures", async () => {
    const onError = vi.fn();
    const clipboard = { writeText: vi.fn(async () => {}), readText: vi.fn(async () => Promise.reject(new Error("denied"))) };
    createClipboardKeyHandler(fakeTerm(), "linux", clipboard, onError)(key("V", { ctrlKey: true, shiftKey: true }));
    await tick();
    expect(onError).toHaveBeenCalledOnce();
  });
});
