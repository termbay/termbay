import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { WebglAddon } from "@xterm/addon-webgl";
import { Terminal, type ITerminalOptions } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";

import type { Disposable, PtySession } from "../transport/types";
import { connectPty } from "./connect";
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

/** One xterm.js instance inside a container, attached to one pty session. */
export class TerminalView implements Disposable {
  readonly term: Terminal;
  readonly search: SearchAddon;
  private readonly fit: FitAddon;
  private readonly renderer: RendererController<WebglAddon>;
  private readonly disposables: Disposable[] = [];

  constructor(
    private readonly container: HTMLElement,
    options: ITerminalOptions = {},
    onRendererChange?: (kind: RendererKind, reason?: unknown) => void,
  ) {
    this.term = new Terminal({ ...DEFAULT_TERMINAL_OPTIONS, ...options });
    this.fit = new FitAddon();
    this.search = new SearchAddon();
    this.term.loadAddon(this.fit);
    this.term.loadAddon(this.search);
    this.term.loadAddon(new Unicode11Addon());
    this.term.unicode.activeVersion = "11";

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
    this.disposables.push({
      dispose: () => {
        clearTimeout(timer);
        observer.disconnect();
      },
    });
  }

  get rendererKind(): RendererKind {
    return this.renderer.kind;
  }

  attach(session: PtySession): Disposable {
    const connection = connectPty(this.term, session);
    this.disposables.push(connection);
    return connection;
  }

  focus(): void {
    this.term.focus();
  }

  private fitNow(): void {
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
