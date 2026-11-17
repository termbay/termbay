import type { Os } from "../platform/os";

export type ClipboardAction = "copy" | "paste";

/** The fields of a `KeyboardEvent` used here. */
export interface KeyEventLike {
  readonly type: string;
  readonly key: string;
  readonly code: string;
  readonly ctrlKey: boolean;
  readonly shiftKey: boolean;
  readonly altKey: boolean;
  readonly metaKey: boolean;
  preventDefault(): void;
}

/** Letter of the key, by layout first; by physical key for non-Latin layouts. */
function letterOf(ev: KeyEventLike): string {
  if (/^[a-z]$/i.test(ev.key)) return ev.key.toLowerCase();
  const match = /^Key([A-Z])$/.exec(ev.code);
  return match?.[1]?.toLowerCase() ?? "";
}

/**
 * Copy/paste shortcuts per OS, chosen so they never shadow shell keys:
 * - macOS: Cmd+C / Cmd+V (Ctrl+C stays with the shell).
 * - Linux and Windows: Ctrl+Shift+C / Ctrl+Shift+V (plain Ctrl+C/Ctrl+V go to the shell).
 */
export function clipboardActionFor(ev: KeyEventLike, os: Os): ClipboardAction | null {
  const modifiersOk =
    os === "macos"
      ? ev.metaKey && !ev.ctrlKey && !ev.altKey && !ev.shiftKey
      : ev.ctrlKey && ev.shiftKey && !ev.altKey && !ev.metaKey;
  if (!modifiersOk) return null;
  const letter = letterOf(ev);
  if (letter === "c") return "copy";
  if (letter === "v") return "paste";
  return null;
}

/** The subset of the xterm.js `Terminal` needed for copy and paste. */
export interface ClipboardTerminal {
  hasSelection(): boolean;
  getSelection(): string;
  /** Sends text as a paste: xterm.js normalizes line endings and adds bracketed paste markers when the shell enabled them. */
  paste(text: string): void;
}

export interface ClipboardLike {
  writeText(text: string): Promise<void>;
  readText(): Promise<string>;
}

/**
 * Builds a handler for `Terminal.attachCustomKeyEventHandler`. Returns false
 * (xterm.js must not process the key) for copy/paste shortcuts, true otherwise.
 *
 * On macOS the native Cmd+C / Cmd+V path is kept: the webview fires `copy` and
 * `paste` events (via the app's Edit menu) and xterm.js handles them, with no
 * clipboard permission prompt. On Linux and Windows Ctrl+Shift+C/V have no
 * native meaning (or open devtools), so the async clipboard API is used.
 */
export function createClipboardKeyHandler(
  term: ClipboardTerminal,
  os: Os,
  clipboard: ClipboardLike | undefined,
  onError: (error: unknown) => void = (error) => console.warn("[termbay] clipboard error", error),
): (ev: KeyEventLike) => boolean {
  return (ev) => {
    const action = clipboardActionFor(ev, os);
    if (action === null) return true;
    if (os === "macos" || ev.type !== "keydown") return false;

    ev.preventDefault();
    if (!clipboard) {
      onError(new Error("clipboard API unavailable"));
      return false;
    }
    if (action === "copy") {
      if (term.hasSelection()) clipboard.writeText(term.getSelection()).catch(onError);
    } else {
      clipboard
        .readText()
        .then((text) => {
          if (text.length > 0) term.paste(text);
        })
        .catch(onError);
    }
    return false;
  };
}
