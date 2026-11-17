import { afterEach, describe, expect, it, vi } from "vitest";
import type { CompletionItem, CompletionResult } from "../transport/types";
import {
  AutocompleteController,
  filterCandidates,
  insertionBytes,
  toCandidate,
  unescapeShell,
  type AutocompleteKeyEvent,
  type MenuState,
} from "./controller";

const enc = (s: string) => new TextEncoder().encode(s);
const dec = (b: Uint8Array) => new TextDecoder().decode(b);
const tick = () => new Promise((r) => setTimeout(r, 0));

function key(
  k: string,
  mods: Partial<Pick<AutocompleteKeyEvent, "ctrlKey" | "shiftKey" | "altKey" | "metaKey" | "isComposing" | "keyCode">> = {},
  type = "keydown",
) {
  return {
    type,
    key: k,
    ctrlKey: false,
    shiftKey: false,
    altKey: false,
    metaKey: false,
    ...mods,
    defaultPrevented: false,
    preventDefault() {
      this.defaultPrevented = true;
    },
  };
}

const item = (text: string, insert: string, kind: CompletionItem["kind"] = "shell", description?: string): CompletionItem => ({
  text,
  insert,
  kind,
  ...(description !== undefined && { description }),
});

/** `git ch` with the cursor at the end: the shell completes the word `ch`. */
const GIT_CH: CompletionResult = {
  status: "ok",
  line: "git ch",
  cursor: 6,
  replaceFrom: 4,
  items: [
    item("checkout", "eckout", "shell", "switch branches"),
    item("cherry-pick", "erry-pick"),
    item("cherry", "erry"),
    item("git checkout main", "eckout main", "history"),
  ],
};

function setup(results: Array<CompletionResult | Promise<CompletionResult> | Error> = [GIT_CH]) {
  const writes: string[] = [];
  const shown: MenuState[] = [];
  let hidden = 0;
  const terminal = { alternate: false, bracketed: false };
  const queue = [...results];
  const complete = vi.fn(async (): Promise<CompletionResult> => {
    const next = queue.shift() ?? { status: "unavailable" };
    if (next instanceof Error) throw next;
    return next;
  });
  const errors: unknown[] = [];
  const controller = new AutocompleteController(
    { isAlternateBuffer: () => terminal.alternate, bracketedPasteMode: () => terminal.bracketed },
    {
      complete,
      write: async (data) => {
        writes.push(dec(data));
      },
    },
    {
      show: (state) => shown.push(state),
      hide: () => {
        hidden++;
      },
    },
    { onError: (e) => errors.push(e) },
  );
  const type = (s: string) => controller.input(enc(s));
  const last = () => shown[shown.length - 1];
  return { controller, writes, shown, last, hidden: () => hidden, terminal, complete, type, errors };
}

async function openMenu(results?: Array<CompletionResult | Error>) {
  const ctx = setup(results);
  const tab = key("Tab");
  expect(ctx.controller.handleKey(tab)).toBe(false);
  expect(tab.defaultPrevented).toBe(true);
  await tick();
  return ctx;
}

afterEach(() => {
  vi.useRealTimers();
});

describe("Tab with the shell at its prompt", () => {
  it("asks for completions and opens the menu, shell items before history", async () => {
    const { controller, shown, complete, writes } = await openMenu();
    expect(complete).toHaveBeenCalledOnce();
    expect(controller.isOpen).toBe(true);
    expect(shown).toHaveLength(1);
    expect(shown[0]?.entries.map((e) => e.text)).toEqual(["checkout", "cherry-pick", "cherry", "git checkout main"]);
    expect(shown[0]?.entries[0]?.description).toBe("switch branches");
    expect(shown[0]?.selected).toBe(0);
    expect(shown[0]?.wordLength).toBe(2);
    expect(writes).toEqual([]);
  });

  it("inserts a single candidate at once, without the menu", async () => {
    const { shown, writes, controller } = await openMenu([
      { status: "ok", line: "ls fix", cursor: 6, replaceFrom: 3, items: [item("fixtures/", "tures/")] },
    ]);
    expect(shown).toEqual([]);
    expect(controller.isOpen).toBe(false);
    expect(writes).toEqual(["tures/"]);
  });

  it("gives Tab to the shell when the core finds no candidate", async () => {
    const { shown, writes, controller } = await openMenu([{ status: "ok", line: "zz", cursor: 2, replaceFrom: 0, items: [] }]);
    expect(shown).toEqual([]);
    expect(controller.isOpen).toBe(false);
    expect(writes).toEqual(["\t"]);
  });

  it("gives Tab to the shell when the core answers unavailable (no integration, not at the prompt)", async () => {
    const { writes, controller, shown } = await openMenu([{ status: "unavailable" }]);
    expect(writes).toEqual(["\t"]);
    expect(controller.isOpen).toBe(false);
    expect(shown).toEqual([]);
  });

  it("gives Tab to the shell when the request fails, and reports the error", async () => {
    const { writes, errors } = await openMenu([new Error("unknown command pty_complete")]);
    expect(writes).toEqual(["\t"]);
    expect(errors).toHaveLength(1);
  });

  it("does not resend Tab after a timeout (the shell already got the request)", async () => {
    const { writes, controller } = await openMenu([{ status: "timeout" }]);
    expect(writes).toEqual([]);
    expect(controller.isOpen).toBe(false);
  });

  it("holds keys typed while waiting and sends them after the Tab, in order", async () => {
    let resolve!: (r: CompletionResult) => void;
    const ctx = setup([new Promise<CompletionResult>((r) => (resolve = r))]);
    ctx.controller.handleKey(key("Tab"));
    ctx.type("x");
    ctx.type("y");
    expect(ctx.writes).toEqual([]);
    resolve({ status: "unavailable" });
    await tick();
    expect(ctx.writes).toEqual(["\t", "x", "y"]);
  });

  it("swallows a second Tab while waiting", async () => {
    let resolve!: (r: CompletionResult) => void;
    const ctx = setup([new Promise<CompletionResult>((r) => (resolve = r))]);
    ctx.controller.handleKey(key("Tab"));
    const second = key("Tab");
    expect(ctx.controller.handleKey(second)).toBe(false);
    expect(second.defaultPrevented).toBe(true);
    expect(ctx.complete).toHaveBeenCalledOnce();
    resolve({ status: "unavailable" });
    await tick();
    expect(ctx.writes).toEqual(["\t"]);
  });

  it("filters with keys typed while waiting once the answer arrives", async () => {
    let resolve!: (r: CompletionResult) => void;
    const ctx = setup([new Promise<CompletionResult>((r) => (resolve = r))]);
    ctx.controller.handleKey(key("Tab"));
    ctx.type("e");
    ctx.type("c");
    resolve(GIT_CH);
    await tick();
    expect(ctx.writes).toEqual(["e", "c"]);
    expect(ctx.last()?.entries.map((e) => e.text)).toEqual(["checkout", "git checkout main"]);
  });

  it("leaves Tab with modifiers to the shell (Shift+Tab, Ctrl+Tab)", () => {
    const { controller, complete } = setup();
    expect(controller.handleKey(key("Tab", { shiftKey: true }))).toBe(true);
    expect(controller.handleKey(key("Tab", { ctrlKey: true }))).toBe(true);
    expect(complete).not.toHaveBeenCalled();
  });

  it("can use Ctrl+Space as the trigger, keeping the native Tab", async () => {
    const writes: string[] = [];
    const complete = vi.fn(async (): Promise<CompletionResult> => ({ status: "unavailable" }));
    const controller = new AutocompleteController(
      { isAlternateBuffer: () => false, bracketedPasteMode: () => false },
      { complete, write: async (d) => void writes.push(dec(d)) },
      { show: () => {}, hide: () => {} },
      { trigger: "ctrl+space" },
    );
    expect(controller.handleKey(key("Tab"))).toBe(true);
    expect(controller.handleKey(key(" ", { ctrlKey: true }))).toBe(false);
    await tick();
    expect(complete).toHaveBeenCalledOnce();
    expect(writes).toEqual(["\x00"]);
  });
});

describe("where the menu never appears", () => {
  it("passes Tab to the program in the alternate buffer (vim, top, less)", () => {
    const { controller, terminal, complete } = setup();
    terminal.alternate = true;
    const tab = key("Tab");
    expect(controller.handleKey(tab)).toBe(true);
    expect(tab.defaultPrevented).toBe(false);
    expect(complete).not.toHaveBeenCalled();
  });

  it("does not intercept keys during IME or dead-key composition", () => {
    const { controller, complete } = setup();
    expect(controller.handleKey(key("Tab", { isComposing: true }))).toBe(true);
    expect(controller.handleKey(key("Tab", { keyCode: 229 }))).toBe(true);
    expect(controller.handleKey(key("Dead"))).toBe(true);
    expect(complete).not.toHaveBeenCalled();
  });

  it("is not opened by pasted text, even with a tab in it", () => {
    const { controller, writes, complete } = setup();
    controller.input(enc("\x1b[200~a\tb\x1b[201~"));
    controller.input(enc("a\tb"));
    expect(complete).not.toHaveBeenCalled();
    expect(writes).toEqual(["\x1b[200~a\tb\x1b[201~", "a\tb"]);
  });

  it("ignores keyup and keypress for the trigger", () => {
    const { controller, complete } = setup();
    expect(controller.handleKey(key("Tab", {}, "keyup"))).toBe(true);
    expect(complete).not.toHaveBeenCalled();
  });
});

describe("keys with the menu open", () => {
  it("moves the selection with arrows and Shift+Tab, wrapping, without reaching the shell", async () => {
    const { controller, last, writes } = await openMenu();
    expect(controller.handleKey(key("ArrowDown"))).toBe(false);
    expect(last()?.selected).toBe(1);
    controller.handleKey(key("ArrowUp"));
    controller.handleKey(key("ArrowUp"));
    expect(last()?.selected).toBe(3);
    controller.handleKey(key("Tab", { shiftKey: true }));
    expect(last()?.selected).toBe(2);
    expect(writes).toEqual([]);
  });

  it("Enter inserts the selection and never sends CR", async () => {
    const { controller, writes } = await openMenu();
    controller.handleKey(key("ArrowDown"));
    const enter = key("Enter");
    expect(controller.handleKey(enter)).toBe(false);
    expect(enter.defaultPrevented).toBe(true);
    expect(controller.isOpen).toBe(false);
    expect(writes).toEqual(["erry-pick"]);
    expect(writes.join("")).not.toMatch(/[\r\n]/);
    // The keypress and keyup of that Enter must not reach xterm.js either.
    expect(controller.handleKey(key("Enter", {}, "keypress"))).toBe(false);
    expect(controller.handleKey(key("Enter", {}, "keyup"))).toBe(false);
    // The next Enter runs the command as usual.
    expect(controller.handleKey(key("Enter"))).toBe(true);
  });

  it("Tab inserts the selection too", async () => {
    const { controller, writes } = await openMenu();
    expect(controller.handleKey(key("Tab"))).toBe(false);
    expect(writes).toEqual(["eckout"]);
  });

  it("inserts a history line as the rest of the line", async () => {
    const { controller, writes } = await openMenu();
    controller.handleKey(key("ArrowUp"));
    controller.handleKey(key("Enter"));
    expect(writes).toEqual(["eckout main"]);
  });

  it("wraps the inserted text in bracketed paste markers when the shell enabled them", async () => {
    const { controller, writes, terminal } = await openMenu();
    terminal.bracketed = true;
    controller.handleKey(key("Enter"));
    expect(writes).toEqual(["\x1b[200~eckout\x1b[201~"]);
  });

  it("Esc closes without sending anything", async () => {
    const { controller, writes, hidden } = await openMenu();
    expect(controller.handleKey(key("Escape"))).toBe(false);
    expect(controller.isOpen).toBe(false);
    expect(hidden()).toBe(1);
    expect(writes).toEqual([]);
    expect(controller.handleKey(key("Escape"))).toBe(true);
  });

  it.each([
    ["ArrowLeft", {}],
    ["ArrowRight", {}],
    ["Home", {}],
    ["c", { ctrlKey: true }],
    ["r", { ctrlKey: true }],
    ["b", { altKey: true }],
    ["k", { metaKey: true }],
  ] as const)("%s %o closes the menu and goes to the shell", async (k, mods) => {
    const { controller } = await openMenu();
    expect(controller.handleKey(key(k, mods))).toBe(true);
    expect(controller.isOpen).toBe(false);
  });

  it("keeps the menu open on a bare modifier key press", async () => {
    const { controller } = await openMenu();
    expect(controller.handleKey(key("Shift", { shiftKey: true }))).toBe(true);
    expect(controller.isOpen).toBe(true);
  });

  it("space closes the menu and reaches the shell", async () => {
    const { controller, type, writes } = await openMenu();
    expect(controller.handleKey(key(" "))).toBe(true);
    type(" ");
    expect(controller.isOpen).toBe(false);
    expect(writes).toEqual([" "]);
  });

  it("closes on paste and lets it through", async () => {
    const { controller, type, writes } = await openMenu();
    type("\x1b[200~foo\x1b[201~");
    expect(controller.isOpen).toBe(false);
    expect(writes).toEqual(["\x1b[200~foo\x1b[201~"]);
  });

  it("close() from outside (blur, new prompt, pty exit) hides the menu", async () => {
    const { controller, hidden } = await openMenu();
    controller.close();
    expect(controller.isOpen).toBe(false);
    expect(hidden()).toBe(1);
  });

  it("pick() inserts the clicked entry", async () => {
    const { controller, writes } = await openMenu();
    controller.pick(2);
    expect(writes).toEqual(["erry"]);
    expect(controller.isOpen).toBe(false);
  });
});

describe("local filter while typing", () => {
  it("sends typed characters to the shell and refines the list without a new query", async () => {
    const { type, writes, last, complete, controller } = await openMenu();
    type("e");
    type("r");
    expect(writes).toEqual(["e", "r"]);
    expect(complete).toHaveBeenCalledOnce();
    expect(last()?.entries.map((e) => e.text)).toEqual(["cherry-pick", "cherry"]);
    expect(last()?.wordLength).toBe(4);
    controller.handleKey(key("Enter"));
    // Only what is still missing after "cher".
    expect(writes.slice(2)).toEqual(["ry-pick"]);
  });

  it("keeps the selected entry selected while it still matches", async () => {
    const { controller, type, last } = await openMenu();
    controller.handleKey(key("ArrowDown")); // cherry-pick
    type("e");
    expect(last()?.entries[last()?.selected ?? -1]?.text).toBe("cherry-pick");
  });

  it("accepts composed characters (pt-BR accents) as filter input", async () => {
    const { type, last, controller } = await openMenu([
      { status: "ok", line: "cd a", cursor: 4, replaceFrom: 3, items: [item("ação/", "ção/"), item("abc/", "bc/")] },
    ]);
    type("ç");
    expect(controller.isOpen).toBe(true);
    expect(last()?.entries.map((e) => e.text)).toEqual(["ação/"]);
  });

  it("closes when nothing matches any more", async () => {
    const { type, controller, writes } = await openMenu();
    type("z");
    expect(controller.isOpen).toBe(false);
    expect(writes).toEqual(["z"]);
  });

  it("Backspace widens the filter, and closes before the start of the word", async () => {
    const { type, last, controller, writes } = await openMenu();
    type("er");
    expect(last()?.entries.map((e) => e.text)).toEqual(["cherry-pick", "cherry"]);
    type("\x7f");
    expect(last()?.entries).toHaveLength(4);
    type("\x7f\x7f\x7f"); // "che" -> "" (one write with three DELs)
    expect(controller.isOpen).toBe(true); // word is empty now
    type("\x7f");
    expect(controller.isOpen).toBe(false);
    expect(writes).toEqual(["er", "\x7f", "\x7f\x7f\x7f", "\x7f"]);
  });

  it("asks again after `/`, with a debounce, and opens the new list", async () => {
    vi.useFakeTimers();
    const ctx = setup([
      { status: "ok", line: "cd s", cursor: 4, replaceFrom: 3, items: [item("src/", "rc/"), item("scripts/", "cripts/")] },
      { status: "ok", line: "cd src/", cursor: 7, replaceFrom: 3, items: [item("src/main.ts", "main.ts"), item("src/ui/", "ui/")] },
    ]);
    ctx.controller.handleKey(key("Tab"));
    await vi.advanceTimersByTimeAsync(0);
    ctx.type("rc");
    ctx.type("/");
    expect(ctx.writes).toEqual(["rc", "/"]);
    expect(ctx.controller.isOpen).toBe(false);
    expect(ctx.complete).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(49);
    expect(ctx.complete).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(ctx.complete).toHaveBeenCalledTimes(2);
    expect(ctx.controller.isOpen).toBe(true);
    expect(ctx.last()?.entries.map((e) => e.text)).toEqual(["src/main.ts", "src/ui/"]);
  });

  it("cancels the new query when a space or Enter follows the `/`", async () => {
    vi.useFakeTimers();
    const ctx = setup([
      { status: "ok", line: "cd s", cursor: 4, replaceFrom: 3, items: [item("src/", "rc/"), item("scripts/", "cripts/")] },
    ]);
    ctx.controller.handleKey(key("Tab"));
    await vi.advanceTimersByTimeAsync(0);
    ctx.type("rc/");
    expect(ctx.controller.handleKey(key("Enter"))).toBe(true);
    ctx.type("\r");
    await vi.advanceTimersByTimeAsync(100);
    expect(ctx.complete).toHaveBeenCalledTimes(1);
    expect(ctx.writes).toEqual(["rc/", "\r"]);
  });
});

describe("candidates that change case or need escaping", () => {
  const PRO: CompletionResult = {
    status: "ok",
    line: "cd pro",
    cursor: 6,
    replaceFrom: 3,
    items: [
      item("Projetos\\ OpenSources/", "\x7f\x7f\x7fProjetos\\ OpenSources/"),
      item("Projects/", "\x7f\x7f\x7fProjects/"),
      item("Pessoal/", "\x7f\x7f\x7fPessoal/"),
    ],
  };

  it("shows the readable name, filters ignoring case and inserts the escaped form", async () => {
    const { shown, last, type, controller, writes } = await openMenu([PRO]);
    expect(shown[0]?.entries.map((e) => e.text)).toEqual(["Projetos OpenSources/", "Projects/", "Pessoal/"]);
    type("j");
    type("e");
    type("t");
    expect(last()?.entries.map((e) => e.text)).toEqual(["Projetos OpenSources/"]);
    controller.handleKey(key("Enter"));
    expect(writes).toEqual(["j", "e", "t", "\x7f".repeat(6) + "Projetos\\ OpenSources/"]);
  });

  it("keeps matching with typed upper case", async () => {
    const { last, type } = await openMenu([PRO]);
    type("J");
    expect(last()?.entries.map((e) => e.text)).toEqual(["Projetos OpenSources/", "Projects/"]);
  });
});

describe("pure helpers", () => {
  it("rebuilds the replacement from the core's insertion plan", () => {
    expect(toCandidate(item("checkout", "eckout"), "ch").replacement).toBe("checkout");
    const fuzzy = toCandidate(item("Checkout", "\x7f\x7fCheckout"), "ch");
    expect(fuzzy.replacement).toBe("Checkout");
    expect(fuzzy.prefixMatch).toBe(false);
  });

  it("strips control characters from labels and replacements", () => {
    const c = toCandidate(item("ev\x1bil\r", "il\r\n\x1b[201~x"), "ev");
    expect(c.text).toBe("evil");
    expect(c.replacement).toBe("evil[201~x");
  });

  it("computes the suffix, or DELs plus the new text, never with CR/LF", () => {
    expect(dec(insertionBytes("ch", "checkout", false))).toBe("eckout");
    expect(dec(insertionBytes("ch", "Checkout", false))).toBe("\x7f\x7fCheckout");
    expect(dec(insertionBytes("aç", "Ação", true))).toBe("\x7f\x7f\x1b[200~Ação\x1b[201~");
    expect(dec(insertionBytes("", "a\r\nb", false))).toBe("ab");
    expect(insertionBytes("checkout", "checkout", true).byteLength).toBe(0);
  });

  it("unescapes shell words for display", () => {
    expect(unescapeShell("Projetos\\ OpenSources/")).toBe("Projetos OpenSources/");
    expect(unescapeShell("a\\\\b")).toBe("a\\b");
    expect(unescapeShell("plain")).toBe("plain");
  });

  it("filters by prefix ignoring case, and by substring for loose matches", () => {
    const list = [
      toCandidate(item("checkout", "eckout"), "ch"),
      toCandidate(item("Cherry", "erry"), "Ch"),
      toCandidate(item("fetch-changes", "\x7f\x7ffetch-changes"), "ch"),
    ];
    expect(filterCandidates(list, "ch", "ch")).toHaveLength(3);
    expect(filterCandidates(list, "che", "ch").map((c) => c.text)).toEqual(["checkout", "Cherry"]);
    expect(filterCandidates(list, "cha", "ch").map((c) => c.text)).toEqual(["fetch-changes"]);
  });
});
