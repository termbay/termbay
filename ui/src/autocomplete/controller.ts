import type { CompletionItem, CompletionResult } from "../transport/types";

/**
 * Key handling and local filtering of the autocomplete menu (docs/AUTOCOMPLETE.md,
 * section 1). Pure TypeScript, no DOM: the terminal, the pty and the menu are
 * reached through the small interfaces below, so every rule is unit-testable.
 */

/** The fields of a `KeyboardEvent` used here. */
export interface AutocompleteKeyEvent {
  readonly type: string;
  readonly key: string;
  readonly ctrlKey: boolean;
  readonly shiftKey: boolean;
  readonly altKey: boolean;
  readonly metaKey: boolean;
  /** True while an IME (or a dead key) is composing a character. */
  readonly isComposing?: boolean;
  /** 229 while an IME is processing the key, on browsers that do not set `isComposing`. */
  readonly keyCode?: number;
  preventDefault(): void;
}

/** What the controller needs to know about the terminal. */
export interface AutocompleteTerminal {
  /** Full-screen programs (vim, top, less) use the alternate buffer: the menu never shows there. */
  isAlternateBuffer(): boolean;
  /** True when the shell enabled bracketed paste (`CSI ? 2004 h`). */
  bracketedPasteMode(): boolean;
}

/** The pty side: completions and raw input. */
export interface CompletionSource {
  complete(): Promise<CompletionResult>;
  write(data: Uint8Array): Promise<void>;
}

export interface MenuEntry {
  readonly text: string;
  readonly description?: string;
  readonly kind: "shell" | "history";
}

export interface MenuState {
  readonly entries: readonly MenuEntry[];
  readonly selected: number;
  /** Length, in characters, of the word being completed (the menu aligns with its start). */
  readonly wordLength: number;
}

/** Where the menu is drawn. */
export interface MenuSink {
  show(state: MenuState): void;
  hide(): void;
}

export type Trigger = "tab" | "ctrl+space";

export interface AutocompleteOptions {
  /** Key that opens the menu (`[autocomplete] trigger`). Defaults to Tab. */
  readonly trigger?: Trigger;
  /** Debounce of the new query after `/`, in ms. */
  readonly requeryDelayMs?: number;
  readonly onError?: (error: unknown) => void;
}

export const REQUERY_DELAY_MS = 50;

const DEL = "\x7f";
const PASTE_START = "\x1b[200~";
const PASTE_END = "\x1b[201~";
/** C0 controls, DEL and C1 controls: never inserted (an ESC could end a bracketed paste). */
const CONTROL_CHARS = /[\x00-\x1f\x7f-\x9f]/g;
/** Characters that keep the menu open and refine the list ("letters, numbers, -, ."). */
const WORD_INPUT = /^[\p{L}\p{M}\p{N}_.\-]+$/u;
/** Same, ending with `/`: next level of a path, which asks the shell again. */
const PATH_INPUT = /^[\p{L}\p{M}\p{N}_.\-]*\/$/u;
const MODIFIER_KEYS = new Set(["Shift", "Control", "Alt", "Meta", "AltGraph", "CapsLock", "OS"]);

const encoder = new TextEncoder();
const decoder = new TextDecoder();

/** An item from the core, with the full text that replaces the word. */
export interface Candidate extends MenuEntry {
  /** Text that replaces the whole word being completed. */
  readonly replacement: string;
  /** False when the shell matched it without a plain prefix (fuzzy, case-insensitive...). */
  readonly prefixMatch: boolean;
}

export function stripControls(text: string): string {
  return text.replace(CONTROL_CHARS, "");
}

function length(text: string): number {
  let n = 0;
  for (const _ of text) n++;
  return n;
}

/** Characters (code points) `from`..`to` of `text`. The core's positions count characters. */
export function sliceChars(text: string, from: number, to: number): string {
  return Array.from(text).slice(from, to).join("");
}

/**
 * Turns the core's insertion plan (suffix, or N x DEL + text, computed for the
 * word the shell saw) back into the full replacement, so the plan can be
 * recomputed after the user typed more characters.
 */
export function toCandidate(item: CompletionItem, word: string): Candidate {
  let deletes = 0;
  while (item.insert[deletes] === DEL) deletes++;
  const rest = stripControls(item.insert.slice(deletes));
  const replacement = deletes > 0 ? rest : word + rest;
  return {
    text: unescapeShell(stripControls(item.text)),
    ...(item.description !== undefined && { description: stripControls(item.description) }),
    kind: item.kind,
    replacement,
    prefixMatch: deletes === 0,
  };
}

/**
 * Local filter while typing with the menu open. With nothing typed since the
 * answer, the shell's list is kept as is. Otherwise a candidate stays when it
 * starts with the word (ignoring case), or, for candidates the shell matched
 * loosely, when it contains it.
 */
export function filterCandidates(candidates: readonly Candidate[], word: string, original: string): Candidate[] {
  if (word === original) return [...candidates];
  const needles = [word.toLowerCase(), unescapeShell(word).toLowerCase()];
  return candidates.filter((c) => {
    // Both the inserted form (may be escaped, e.g. `Projetos\ OpenSources/`)
    // and the readable form shown in the menu count, ignoring case.
    const hays = [c.replacement.toLowerCase(), unescapeShell(c.replacement).toLowerCase(), c.text.toLowerCase()];
    return needles.some((needle) =>
      hays.some((hay) => hay.startsWith(needle) || (!c.prefixMatch && hay.includes(needle))),
    );
  });
}

/**
 * Readable form of a shell word for display and matching: drops backslash
 * escapes (`Projetos\ OpenSources/` -> `Projetos OpenSources/`). The escaped
 * form is still what gets inserted.
 */
export function unescapeShell(text: string): string {
  return text.replace(/\\(.)/gu, "$1");
}

/**
 * Bytes that turn `word` into `replacement` at the cursor: only the missing
 * suffix when possible, otherwise one DEL per character and the new text. The
 * text goes inside bracketed paste markers when the shell enabled them, so no
 * character can trigger a shell binding. Never contains CR or LF.
 */
export function insertionBytes(word: string, replacement: string, bracketedPaste: boolean): Uint8Array {
  const clean = stripControls(replacement);
  let deletes = 0;
  let text = clean;
  if (clean.startsWith(word)) text = clean.slice(word.length);
  else deletes = length(word);
  let out = DEL.repeat(deletes);
  if (text.length > 0) out += bracketedPaste ? PASTE_START + text + PASTE_END : text;
  return encoder.encode(out);
}

function triggerBytes(trigger: Trigger): Uint8Array {
  // What the key sends to the shell when the menu does not take it.
  return trigger === "tab" ? Uint8Array.of(0x09) : Uint8Array.of(0x00);
}

type State =
  | { readonly kind: "idle" }
  /** After `/`: the menu is hidden and a new query starts after a short debounce. */
  | { readonly kind: "waiting" }
  /** Waiting for `complete`. Input is held so the shell sees it in order. */
  | { readonly kind: "pending"; readonly generation: number; readonly origin: "key" | "requery" }
  | {
      readonly kind: "open";
      readonly candidates: readonly Candidate[];
      /** Word the shell saw. */
      readonly original: string;
      word: string;
      visible: Candidate[];
      selected: number;
    };

/**
 * One per terminal pane, created once the shell integration is active.
 * - `handleKey` goes into `Terminal.attachCustomKeyEventHandler`; false means
 *   xterm.js must not process the key.
 * - `input` replaces the pty write: every byte the terminal sends passes here.
 */
export class AutocompleteController {
  private state: State = { kind: "idle" };
  private generation = 0;
  private held: Uint8Array[] = [];
  private requeryTimer: ReturnType<typeof setTimeout> | undefined;
  private disposed = false;
  /**
   * Key whose keydown was taken. Its keypress/keyup are swallowed too: a
   * keypress for Enter reaching xterm.js after the menu closed would send CR
   * and run the command. Browsers skip keypress after `preventDefault`; this
   * is the belt to those braces.
   */
  private taken: string | null = null;
  private readonly trigger: Trigger;
  private readonly requeryDelayMs: number;
  private readonly onError: (error: unknown) => void;

  constructor(
    private readonly terminal: AutocompleteTerminal,
    private readonly source: CompletionSource,
    private readonly menu: MenuSink,
    options: AutocompleteOptions = {},
  ) {
    this.trigger = options.trigger ?? "tab";
    this.requeryDelayMs = options.requeryDelayMs ?? REQUERY_DELAY_MS;
    this.onError = options.onError ?? ((error) => console.error("[termbay] autocomplete error", error));
  }

  get isOpen(): boolean {
    return this.state.kind === "open";
  }

  get isPending(): boolean {
    return this.state.kind === "pending";
  }

  handleKey(ev: AutocompleteKeyEvent): boolean {
    if (this.disposed) return true;
    if (ev.type !== "keydown" && this.taken !== null && ev.key === this.taken) {
      if (ev.type === "keyup") this.taken = null;
      return false;
    }
    if (ev.type === "keydown") this.taken = null;
    // Composition (accents, dead keys, IME): never intercepted; the composed
    // text arrives through `input` and filters like any typed character.
    if (ev.isComposing || ev.keyCode === 229 || ev.key === "Dead" || ev.key === "Process") return true;

    const state = this.state;
    const isTrigger = this.isTrigger(ev);

    if (state.kind === "open") return this.handleOpenKey(ev, isTrigger);

    if (!isTrigger) return true;
    if (state.kind === "waiting") this.cancelRequery();
    if (state.kind === "pending") {
      // A second trigger while waiting is swallowed: the first answer decides.
      if (ev.type === "keydown") {
        ev.preventDefault();
        this.taken = ev.key;
      }
      return false;
    }
    if (ev.type !== "keydown") return true;
    if (this.terminal.isAlternateBuffer()) return true;
    ev.preventDefault(); // Tab would move the browser focus.
    this.taken = ev.key;
    this.request("key");
    return false;
  }

  private isTrigger(ev: AutocompleteKeyEvent): boolean {
    if (ev.altKey || ev.metaKey) return false;
    if (this.trigger === "tab") return ev.key === "Tab" && !ev.ctrlKey && !ev.shiftKey;
    return ev.key === " " && ev.ctrlKey && !ev.shiftKey;
  }

  private handleOpenKey(ev: AutocompleteKeyEvent, isTrigger: boolean): boolean {
    const keydown = ev.type === "keydown";
    const take = (action: () => void): boolean => {
      if (keydown) {
        ev.preventDefault();
        this.taken = ev.key;
        action();
      }
      return false;
    };
    const plain = !ev.ctrlKey && !ev.altKey && !ev.metaKey;

    if (plain && ev.key === "ArrowDown") return take(() => this.move(1));
    if (plain && ev.key === "ArrowUp") return take(() => this.move(-1));
    if (plain && ev.key === "Tab" && ev.shiftKey) return take(() => this.move(-1));
    if (isTrigger || (plain && !ev.shiftKey && (ev.key === "Enter" || ev.key === "Tab"))) {
      return take(() => this.accept());
    }
    if (plain && ev.key === "Escape") return take(() => this.close());

    if (MODIFIER_KEYS.has(ev.key)) return true;
    // Ctrl/Alt/Cmd + anything, and named keys other than Backspace (arrows
    // left/right, Home, End, PageUp...): close and let the key reach the shell.
    if (!plain || (ev.key.length > 1 && ev.key !== "Backspace")) {
      if (keydown) this.close();
      return true;
    }
    // Printable keys, space and Backspace go to the shell; `input` decides.
    return true;
  }

  /** Input from the terminal (keys, paste, composed text) on its way to the pty. */
  input(data: Uint8Array): void {
    if (this.disposed) {
      this.send(data);
      return;
    }
    const state = this.state;
    if (state.kind === "idle") {
      this.send(data);
      return;
    }
    if (state.kind === "waiting") {
      // More of the path keeps the query scheduled; anything else cancels it.
      const text = decoder.decode(data);
      if (!WORD_INPUT.test(text) && !PATH_INPUT.test(text)) this.cancelRequery();
      this.send(data);
      return;
    }
    if (state.kind === "pending") {
      this.held.push(data);
      return;
    }

    const text = decoder.decode(data);
    if (text.includes("�")) {
      this.close();
      this.send(data);
      return;
    }
    if (/^\x7f+$/.test(text)) {
      this.send(data);
      for (let i = 0; i < text.length && this.state.kind === "open"; i++) {
        // Backspace before the start of the word closes the menu.
        if (state.word.length === 0) this.close();
        else this.refilter(sliceChars(state.word, 0, length(state.word) - 1));
      }
      return;
    }
    if (WORD_INPUT.test(text)) {
      this.send(data);
      this.refilter(state.word + text);
      return;
    }
    if (PATH_INPUT.test(text)) {
      this.send(data);
      this.menu.hide();
      this.state = { kind: "waiting" };
      this.scheduleRequery();
      return;
    }
    // Space, paste, escape sequences, anything else: close and pass through.
    this.close();
    this.send(data);
  }

  /** Closes the menu without changing anything (Esc, blur, new prompt, pty exit...). */
  close(): void {
    this.cancelRequery();
    const wasPending = this.state.kind === "pending";
    if (this.state.kind === "open") this.menu.hide();
    this.state = { kind: "idle" };
    if (wasPending) {
      // The late answer is ignored; held keys still reach the shell, in order.
      this.generation++;
      this.flushHeld();
    }
  }

  /** Inserts the entry at `index` (mouse click on the menu). */
  pick(index: number): void {
    if (this.state.kind !== "open" || index < 0 || index >= this.state.visible.length) return;
    this.state.selected = index;
    this.accept();
  }

  dispose(): void {
    if (this.disposed) return;
    this.close();
    this.disposed = true;
  }

  private request(origin: "key" | "requery"): void {
    const generation = ++this.generation;
    if (this.state.kind === "open") this.menu.hide();
    this.state = { kind: "pending", generation, origin };
    this.source.complete().then(
      (result) => this.receive(generation, origin, result),
      (error: unknown) => {
        if (generation !== this.generation || this.disposed) return;
        this.onError(error);
        // Keep the terminal usable: the key goes to the shell as if we were not here.
        this.finish(origin === "key" ? triggerBytes(this.trigger) : null);
      },
    );
  }

  private receive(generation: number, origin: "key" | "requery", result: CompletionResult): void {
    if (generation !== this.generation || this.disposed) return;
    if (result.status === "unavailable") {
      // Not at the prompt, or no integration: Tab belongs to the shell.
      this.finish(origin === "key" ? triggerBytes(this.trigger) : null);
      return;
    }
    if (result.status !== "ok") {
      // Timeout (or an unknown status): the shell already got the request; do nothing.
      this.finish(null);
      return;
    }

    const original = sliceChars(result.line, result.replaceFrom, result.cursor);
    const candidates = orderBySection(result.items.map((item) => toCandidate(item, original)));
    if (candidates.length === 0) {
      // Nothing from the core: let the shell's own Tab act (it may still beep or list).
      this.finish(origin === "key" ? triggerBytes(this.trigger) : null);
      return;
    }
    if (origin === "key" && candidates.length === 1) {
      const only = candidates[0] as Candidate;
      this.finish(insertionBytes(original, only.replacement, this.terminal.bracketedPasteMode()));
      return;
    }
    this.state = { kind: "open", candidates, original, word: original, visible: [...candidates], selected: 0 };
    this.render();
    this.flushHeld();
  }

  /** Ends a request: optional bytes first, then the keys held meanwhile. */
  private finish(bytes: Uint8Array | null): void {
    this.state = { kind: "idle" };
    if (bytes && bytes.byteLength > 0) this.send(bytes);
    this.flushHeld();
  }

  private flushHeld(): void {
    const held = this.held;
    this.held = [];
    for (const data of held) this.input(data);
  }

  private refilter(word: string): void {
    const state = this.state;
    if (state.kind !== "open") return;
    const previous = state.visible[state.selected];
    state.word = word;
    state.visible = filterCandidates(state.candidates, word, state.original);
    if (state.visible.length === 0) {
      this.close();
      return;
    }
    const kept = previous ? state.visible.indexOf(previous) : -1;
    state.selected = kept >= 0 ? kept : 0;
    this.render();
  }

  private scheduleRequery(): void {
    clearTimeout(this.requeryTimer);
    this.requeryTimer = setTimeout(() => {
      this.requeryTimer = undefined;
      if (this.state.kind !== "waiting") return;
      if (this.terminal.isAlternateBuffer()) this.state = { kind: "idle" };
      else this.request("requery");
    }, this.requeryDelayMs);
  }

  private cancelRequery(): void {
    clearTimeout(this.requeryTimer);
    this.requeryTimer = undefined;
    if (this.state.kind === "waiting") this.state = { kind: "idle" };
  }

  private move(delta: number): void {
    const state = this.state;
    if (state.kind !== "open" || state.visible.length === 0) return;
    const n = state.visible.length;
    state.selected = (state.selected + delta + n) % n;
    this.render();
  }

  private accept(): void {
    const state = this.state;
    if (state.kind !== "open") return;
    const chosen = state.visible[state.selected];
    this.menu.hide();
    this.state = { kind: "idle" };
    if (chosen) this.send(insertionBytes(state.word, chosen.replacement, this.terminal.bracketedPasteMode()));
  }

  private render(): void {
    const state = this.state;
    if (state.kind !== "open") return;
    this.menu.show({
      entries: state.visible,
      selected: state.selected,
      wordLength: length(state.word),
    });
  }

  private send(data: Uint8Array): void {
    this.source.write(data).catch(this.onError);
  }
}

/** Shell completions first, then history, keeping the core's order inside each. */
function orderBySection(candidates: Candidate[]): Candidate[] {
  return [...candidates.filter((c) => c.kind !== "history"), ...candidates.filter((c) => c.kind === "history")];
}
