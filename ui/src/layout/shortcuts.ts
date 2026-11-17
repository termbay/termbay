import type { Os } from "../platform/os";
import type { Disposable } from "../transport/types";
import type { FocusDirection } from "./model";

/**
 * App shortcuts (docs/FASE1-PLANO.md, section 6.6). macOS uses Cmd; Linux and
 * Windows use Ctrl+Shift. Never plain Ctrl+letter (shell), never Alt/Option+key
 * (Meta in the shell, characters on macOS and ABNT2), never Ctrl+Alt (AltGr).
 */
export type ShortcutAction =
  | { readonly type: "newTab" }
  | { readonly type: "closePane" }
  | { readonly type: "split"; readonly direction: "right" | "down" }
  | { readonly type: "focus"; readonly direction: FocusDirection }
  | { readonly type: "nextTab" }
  | { readonly type: "prevTab" }
  | { readonly type: "goToTab"; readonly index: number }
  /** Reserved: scrollback search (T17). Consumed, does nothing yet. */
  | { readonly type: "find" }
  /** Collapse or expand the vertical tab list. */
  | { readonly type: "toggleTabs" }
  /** Reserved: office panel (phase 2). Consumed, does nothing yet. */
  | { readonly type: "toggleOffice" };

/** The fields of a `KeyboardEvent` used here. */
export interface ShortcutKeyEvent {
  readonly type: string;
  readonly key: string;
  readonly code: string;
  readonly ctrlKey: boolean;
  readonly shiftKey: boolean;
  readonly altKey: boolean;
  readonly metaKey: boolean;
  readonly repeat?: boolean;
  readonly isComposing?: boolean;
  readonly keyCode?: number;
}

/** Letter by layout first; by physical key for non-Latin layouts (as in clipboard.ts). */
function letterOf(ev: ShortcutKeyEvent): string {
  if (/^[a-z]$/i.test(ev.key)) return ev.key.toLowerCase();
  return /^Key([A-Z])$/.exec(ev.code)?.[1]?.toLowerCase() ?? "";
}

/** Digit always by physical key: Shift changes the character. */
function digitOf(ev: ShortcutKeyEvent): number | null {
  const match = /^Digit([1-9])$/.exec(ev.code);
  return match ? Number(match[1]) : null;
}

const ARROWS: Readonly<Record<string, FocusDirection>> = {
  ArrowLeft: "left",
  ArrowRight: "right",
  ArrowUp: "up",
  ArrowDown: "down",
};

/** Actions that create or close: auto-repeat is ignored so a held key does not open 30 tabs. */
const NO_REPEAT = new Set<ShortcutAction["type"]>(["newTab", "closePane", "split"]);

function resolve(ev: ShortcutKeyEvent, os: Os): ShortcutAction | null {
  const { ctrlKey: ctrl, shiftKey: shift, altKey: alt, metaKey: meta } = ev;

  // Ctrl+Tab / Ctrl+Shift+Tab on every OS.
  if (ev.key === "Tab" && ctrl && !alt && !meta) return shift ? { type: "prevTab" } : { type: "nextTab" };

  if (os === "macos") {
    if (!meta || ctrl) return null;
    const arrow = ARROWS[ev.key];
    if (alt) return arrow && !shift ? { type: "focus", direction: arrow } : null;
    if (shift) {
      if (ev.code === "BracketRight") return { type: "nextTab" };
      if (ev.code === "BracketLeft") return { type: "prevTab" };
      const letter = letterOf(ev);
      if (letter === "d") return { type: "split", direction: "down" };
      if (letter === "o") return { type: "toggleOffice" };
      if (letter === "b") return { type: "toggleTabs" };
      return null;
    }
    const digit = digitOf(ev);
    if (digit !== null) return { type: "goToTab", index: digit };
    switch (letterOf(ev)) {
      case "t":
        return { type: "newTab" };
      case "w":
        return { type: "closePane" };
      case "d":
        return { type: "split", direction: "right" };
      case "f":
        return { type: "find" };
      default:
        return null;
    }
  }

  if (!ctrl || !shift || alt || meta) return null;
  const arrow = ARROWS[ev.key];
  if (arrow) return { type: "focus", direction: arrow };
  const digit = digitOf(ev);
  if (digit !== null) return { type: "goToTab", index: digit };
  switch (letterOf(ev)) {
    case "t":
      return { type: "newTab" };
    case "w":
      return { type: "closePane" };
    case "d":
      return { type: "split", direction: "right" };
    case "s":
      return { type: "split", direction: "down" };
    case "f":
      return { type: "find" };
    case "o":
      return { type: "toggleOffice" };
    case "b":
      return { type: "toggleTabs" };
    default:
      return null;
  }
}

/**
 * The app shortcut for a key event, or null when the key belongs to the
 * terminal. IME composition, dead keys and auto-repeat of create/close are
 * ignored.
 */
export function resolveShortcut(ev: ShortcutKeyEvent, os: Os): ShortcutAction | null {
  const action = resolveIgnoringRepeat(ev, os);
  if (action && ev.repeat && NO_REPEAT.has(action.type)) return null;
  return action;
}

function resolveIgnoringRepeat(ev: ShortcutKeyEvent, os: Os): ShortcutAction | null {
  if (ev.type !== "keydown" || ev.isComposing || ev.keyCode === 229) return null;
  return resolve(ev, os);
}

/**
 * True for browser keys whose default action (reload, print, devtools, find)
 * must not run in the webview (mainly WebView2). Only the default is
 * prevented: the event still reaches xterm.js, so Ctrl+R stays the shell's
 * history search and F5 reaches full-screen programs.
 */
export function isBrowserKey(ev: ShortcutKeyEvent): boolean {
  if (ev.type !== "keydown") return false;
  if (ev.key === "F5" || ev.key === "F7" || ev.key === "F12") return true;
  if (!ev.ctrlKey || ev.altKey || ev.metaKey) return false;
  const letter = letterOf(ev);
  if (!ev.shiftKey) return letter === "r" || letter === "p" || letter === "f" || letter === "g" || letter === "u";
  return letter === "r" || letter === "i" || letter === "j" || letter === "p";
}

export interface ShortcutTarget {
  addEventListener(type: "keydown", listener: (ev: KeyboardEvent) => void, options: { capture: boolean }): void;
  removeEventListener(type: "keydown", listener: (ev: KeyboardEvent) => void, options: { capture: boolean }): void;
}

/**
 * One `keydown` listener on the window, in the capture phase, so app shortcuts
 * never reach xterm.js or the autocomplete menu. A repeated create/close key
 * is swallowed too, without acting.
 */
export function installShortcuts(
  target: ShortcutTarget,
  os: Os,
  run: (action: ShortcutAction) => void,
): Disposable {
  const listener = (ev: KeyboardEvent) => {
    const shortcut = resolveIgnoringRepeat(ev, os);
    if (shortcut) {
      ev.preventDefault();
      ev.stopPropagation();
      const action = resolveShortcut(ev, os);
      if (action) run(action);
      return;
    }
    if (os === "windows" && isBrowserKey(ev)) ev.preventDefault();
  };
  target.addEventListener("keydown", listener, { capture: true });
  return { dispose: () => target.removeEventListener("keydown", listener, { capture: true }) };
}
