import type { Terminal } from "@xterm/xterm";

import type {
  CompletionResult,
  Disposable,
  PtyExit,
  PtyInfo,
  PtySession,
  ShellInfo,
} from "../transport/types";
import { AutocompleteController, type AutocompleteKeyEvent, type AutocompleteOptions } from "./controller";
import { CompletionMenu } from "./menu";

/** Integration protocol versions this UI understands. */
export const SUPPORTED_PROTOCOL = 1;

export type KeyHandler = (ev: AutocompleteKeyEvent) => boolean;

export interface AutocompleteHost {
  readonly term: Terminal;
  /** The pane: the menu is positioned inside it. */
  readonly container: HTMLElement;
  /** Installs (or removes) the autocomplete key handler in the pane's key handler chain. */
  setKeyHandler(handler: KeyHandler | null): void;
}

/**
 * A session whose input goes through the autocomplete controller once it
 * exists. Until then (and always, when autocomplete is off) writes go straight
 * to the pty.
 */
export class AutocompleteSession implements PtySession {
  /** @internal */
  controller: AutocompleteController | null = null;

  constructor(private readonly inner: PtySession) {}

  get id(): string {
    return this.inner.id;
  }

  write(data: Uint8Array): Promise<void> {
    if (!this.controller) return this.inner.write(data);
    this.controller.input(data);
    return Promise.resolve();
  }

  resize(cols: number, rows: number): Promise<void> {
    return this.inner.resize(cols, rows);
  }

  ack(bytes: number): void {
    this.inner.ack(bytes);
  }

  close(): Promise<void> {
    return this.inner.close();
  }

  onData(listener: (chunk: Uint8Array) => void): Disposable {
    return this.inner.onData(listener);
  }

  onExit(listener: (exit: PtyExit) => void): Disposable {
    return this.inner.onExit(listener);
  }

  complete(): Promise<CompletionResult> {
    return this.inner.complete();
  }

  onShell(listener: (info: ShellInfo) => void): Disposable {
    return this.inner.onShell(listener);
  }

  info(): Promise<PtyInfo> {
    return this.inner.info();
  }
}

/**
 * Sets up autocomplete for one pane. Nothing is created (no key handler, no
 * DOM) until the core reports active shell integration with `pty-shell`, which
 * only happens when `[autocomplete] enabled = true` and the shell is supported.
 * Returns the session the terminal must write to.
 */
export function attachAutocomplete(
  host: AutocompleteHost,
  session: PtySession,
  options: AutocompleteOptions = {},
): { session: PtySession } & Disposable {
  const gated = new AutocompleteSession(session);
  const disposables: Disposable[] = [];
  let active = false;

  const activate = (info: ShellInfo) => {
    if (active) return;
    if (info.protocol !== SUPPORTED_PROTOCOL) {
      console.warn(`[termbay] unsupported shell integration protocol ${info.protocol}; autocomplete stays off`);
      return;
    }
    active = true;
    const { term, container } = host;
    let controller: AutocompleteController | null = null;
    const menu = new CompletionMenu(container, term, (index) => controller?.pick(index));
    controller = new AutocompleteController(
      {
        isAlternateBuffer: () => term.buffer.active.type === "alternate",
        bracketedPasteMode: () => term.modes.bracketedPasteMode,
      },
      { complete: () => session.complete(), write: (data) => session.write(data) },
      menu,
      options,
    );
    const ctl = controller;
    gated.controller = ctl;
    host.setKeyHandler((ev) => ctl.handleKey(ev));

    const close = () => ctl.close();
    const reposition = () => menu.reposition();
    const textarea = term.textarea;
    textarea?.addEventListener("blur", close);
    const resizeObserver = new ResizeObserver(reposition);
    resizeObserver.observe(container);

    disposables.push(
      term.onCursorMove(reposition),
      term.onScroll(reposition),
      term.onResize(reposition),
      term.buffer.onBufferChange(close),
      // OSC 133 C (command running) and D (command finished) mean the line is
      // gone. Not consumed: the marks stay available to other handlers. A (prompt
      // start) is ignored on purpose, since a shell may repaint its prompt while
      // the menu opens.
      term.parser.registerOscHandler(133, (data) => {
        if (data.startsWith("C") || data.startsWith("D")) close();
        return false;
      }),
      session.onExit(close),
      {
        dispose: () => {
          textarea?.removeEventListener("blur", close);
          resizeObserver.disconnect();
          host.setKeyHandler(null);
          gated.controller = null;
          ctl.dispose();
          menu.dispose();
        },
      },
    );
  };

  disposables.push(session.onShell(activate));

  return {
    session: gated,
    dispose: () => {
      for (const d of disposables.splice(0).reverse()) d.dispose();
    },
  };
}
