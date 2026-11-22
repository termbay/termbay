import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { WebglAddon } from "@xterm/addon-webgl";
import { Terminal, type ITerminalOptions } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";

import { attachAutocomplete, type KeyHandler } from "../autocomplete/attach";
import type { Trigger } from "../autocomplete/controller";
import type { PaneView } from "../layout/pane";
import { cwdLabelFromOsc7, sanitizeTitle } from "../layout/title";
import { detectOs, type Os } from "../platform/os";
import type { Disposable, PtySession } from "../transport/types";
import { createClipboardKeyHandler } from "./clipboard";
import { bufferEarlyInput, connectPty, type EarlyInput } from "./connect";
import { RendererController, type RendererKind } from "./renderer";

const RESIZE_DEBOUNCE_MS = 50;

export const DEFAULT_TERMINAL_OPTIONS: ITerminalOptions = {
  allowProposedApi: true, // required by the unicode11 addon
  cursorBlink: true,
  fontFamily: 'ui-monospace, "SF Mono", Menlo, Consolas, "DejaVu Sans Mono", monospace',
  fontSize: 14,
  scrollback: 10_000,
  // Temporary palette until T15 brings our own themes from the config file.
  theme: { background: "#16181d", foreground: "#d6d9e0", cursor: "#d6d9e0" },
};

/** One xterm.js instance inside a container (a pane), attached to one pty session. */
export class TerminalView implements PaneView {
  readonly term: Terminal;
  readonly search: SearchAddon;
  private readonly fit: FitAddon;
  private readonly renderer: RendererController<WebglAddon>;
  private readonly disposables: Disposable[] = [];
  /** Autocomplete key handler, set only while shell integration is active. */
  private autocompleteKeys: KeyHandler | null = null;
  /** Set when the shell ended and the pane stays open (D20): only Enter acts. */
  private exitedEnter: (() => void) | null = null;
  /** What is typed before `attach`, so the first keystrokes in a new pane are not lost. */
  private readonly earlyInput: EarlyInput;

  constructor(
    private readonly container: HTMLElement,
    options: ITerminalOptions = {},
    onRendererChange?: (kind: RendererKind, reason?: unknown) => void,
    os: Os = detectOs(),
  ) {
    this.term = new Terminal({ ...DEFAULT_TERMINAL_OPTIONS, ...options });
    this.fit = new FitAddon();
    this.search = new SearchAddon();
    this.term.loadAddon(this.fit);
    this.term.loadAddon(this.search);
    this.term.loadAddon(new Unicode11Addon());
    this.term.unicode.activeVersion = "11";

    const clipboardKeys = createClipboardKeyHandler(this.term, os, globalThis.navigator?.clipboard);
    // Autocomplete first: with its menu open, Ctrl/Cmd+key closes it and goes on.
    this.term.attachCustomKeyEventHandler((ev) => {
      if (this.exitedEnter) {
        if (ev.type === "keydown" && ev.key === "Enter" && !ev.ctrlKey && !ev.altKey && !ev.metaKey) this.exitedEnter();
        return false;
      }
      return (this.autocompleteKeys?.(ev) ?? true) && clipboardKeys(ev);
    });
    this.earlyInput = bufferEarlyInput(this.term);
    this.disposables.push(this.earlyInput);
    this.term.open(container);
    // WebGL needs the terminal attached to the DOM first.
    this.renderer = new RendererController<WebglAddon>(this.term, () => new WebglAddon(), onRendererChange);
    this.renderer.start();
    this.fitNow();

    let timer: ReturnType<typeof setTimeout> | undefined;
    const observer = new ResizeObserver(() => {
      clearTimeout(timer);
      timer = setTimeout(() => this.fitNow(), RESIZE_DEBOUNCE_MS);
    });
    observer.observe(container);

    // Clicking anywhere in the pane (including its padding) focuses the terminal.
    const focusOnClick = () => this.term.focus();
    container.addEventListener("mousedown", focusOnClick);

    this.disposables.push({
      dispose: () => {
        clearTimeout(timer);
        observer.disconnect();
        container.removeEventListener("mousedown", focusOnClick);
      },
    });
  }

  get rendererKind(): RendererKind {
    return this.renderer.kind;
  }

  get element(): HTMLElement {
    return this.container;
  }

  get cols(): number {
    return this.term.cols;
  }

  get rows(): number {
    return this.term.rows;
  }

  /** WebGL only while the pane's tab is visible; output keeps being processed. */
  setVisible(visible: boolean): void {
    if (visible) this.renderer.start();
    else this.renderer.stop();
  }

  /** OSC 0/2 title, already sanitized. */
  onTitle(listener: (title: string | null) => void): Disposable {
    return this.term.onTitleChange((raw) => listener(sanitizeTitle(raw)));
  }

  /** Folder label from OSC 7. Display only, never used to open a pty (D23). Not consumed. */
  onCwdLabel(listener: (label: string | null) => void): Disposable {
    return this.term.parser.registerOscHandler(7, (data) => {
      listener(cwdLabelFromOsc7(data));
      return false;
    });
  }

  write(text: string): void {
    this.term.write(text);
  }

  setExited(onEnter: () => void): void {
    this.autocompleteKeys = null;
    // The shell never started (or ended): nothing will read what was typed.
    this.earlyInput.dispose();
    this.exitedEnter = onEnter;
  }

  attach(session: PtySession, options: { trigger?: Trigger } = {}): Disposable {
    const autocomplete = attachAutocomplete(
      {
        term: this.term,
        container: this.container,
        setKeyHandler: (handler) => {
          this.autocompleteKeys = handler;
        },
      },
      session,
      options.trigger ? { trigger: options.trigger } : {},
    );
    const connection = connectPty(this.term, autocomplete.session, { earlyInput: this.earlyInput.take() });
    const both: Disposable = {
      dispose: () => {
        connection.dispose();
        autocomplete.dispose();
      },
    };
    this.disposables.push(both);
    return both;
  }

  focus(): void {
    this.term.focus();
  }

  /** Refits now (e.g. after the pane moved in the layout). */
  fitNow(): void {
    // A hidden container has no size; fitting it would collapse the pty.
    if (this.container.clientWidth === 0 || this.container.clientHeight === 0) return;
    this.fit.fit();
  }

  dispose(): void {
    for (const d of this.disposables.splice(0)) d.dispose();
    this.renderer.dispose();
    this.term.dispose();
  }
}
