import type { Trigger } from "../autocomplete/controller";
import type { Disposable, PtyExit, PtySession, PtyTransport } from "../transport/types";
import {
  closePane,
  closeTab,
  createLayout,
  focus,
  focusPane,
  goToTab,
  leaves,
  newTab,
  nextTab,
  paneExited,
  prevTab,
  setRatio,
  split,
  updatePane,
  type Effect,
  type Layout,
  type PaneId,
  type Result,
} from "./model";
import type { PaneView } from "./pane";
import type { ShortcutAction } from "./shortcuts";
import { sanitizeTitle } from "./title";

export interface LayoutRenderer {
  render(layout: Layout, panes: ReadonlyMap<PaneId, PaneView>): void;
  /** Collapses or expands the tab list. */
  toggleTabs?(): void;
}

export interface LayoutControllerDeps {
  readonly transport: PtyTransport;
  /** Creates the pane element (not yet in the document). */
  createElement(): HTMLElement;
  /** Creates the terminal of a pane inside its element, once the element is placed. */
  createPane(element: HTMLElement): PaneView;
  readonly renderer: LayoutRenderer;
  /** Autocomplete trigger from `get_config` (D22). Defaults to the controller's own default (Tab). */
  readonly trigger?: Trigger;
  /** Asks the user (D21). Resolves true to go on. */
  confirm(message: string): Promise<boolean>;
  /** The last tab closed (D19). */
  closeWindow(): void;
  onError?(error: unknown): void;
}

interface Live {
  readonly element: HTMLElement;
  view: PaneView | null;
  session: PtySession | null;
  subscriptions: Disposable[];
}

const ENTER_HINT = "Press Enter to close the pane.\r\n";

/**
 * Applies the effects of the layout model (model.ts) to the DOM and the
 * transport, and owns the lifecycle of each pane: opening in the directory of
 * the focused shell (D23), titles, shell exit (D20), confirmation (D21) and
 * closing the window with the last tab (D19).
 */
export class LayoutController implements Disposable {
  private state: Layout;
  private readonly live = new Map<PaneId, Live>();
  private trigger: Trigger | undefined;
  private confirming = false;
  private disposed = false;

  constructor(private readonly deps: LayoutControllerDeps) {
    this.trigger = deps.trigger;
    const initial = createLayout();
    this.state = initial.layout;
    this.apply(initial);
  }

  get layout(): Layout {
    return this.state;
  }

  /** The pane's terminal (for tests and the app). */
  paneView(id: PaneId): PaneView | null {
    return this.live.get(id)?.view ?? null;
  }

  session(id: PaneId): PtySession | null {
    return this.live.get(id)?.session ?? null;
  }

  /** Autocomplete trigger from `get_config`; applies to panes opened afterwards. */
  setTrigger(trigger: Trigger): void {
    this.trigger = trigger;
  }

  run(action: ShortcutAction): void {
    if (this.confirming || this.disposed) return;
    switch (action.type) {
      case "newTab":
        return this.dispatch(newTab(this.state));
      case "closePane":
        void this.requestClosePane(this.focusedId());
        return;
      case "split":
        return this.dispatch(split(this.state, action.direction));
      case "focus":
        return this.dispatch(focus(this.state, action.direction));
      case "nextTab":
        return this.dispatch(nextTab(this.state));
      case "prevTab":
        return this.dispatch(prevTab(this.state));
      case "goToTab":
        return this.dispatch(goToTab(this.state, action.index));
      case "toggleTabs":
        this.deps.renderer.toggleTabs?.();
        return;
      case "find":
      case "toggleOffice":
        return; // Reserved (T17, phase 2).
    }
  }

  activateTab(tabId: string): void {
    const tab = this.state.tabs.find((t) => t.id === tabId);
    if (tab) this.dispatch(focusPane(this.state, tab.focused));
  }

  focusPane(id: PaneId): void {
    const tab = this.state.tabs.find((t) => t.id === this.state.active);
    if (tab?.focused === id) return;
    this.dispatch(focusPane(this.state, id));
  }

  setRatio(splitId: string, ratio: number): void {
    this.dispatch(setRatio(this.state, splitId, ratio));
  }

  /** Closes a pane, asking first when a program runs in it (D21). Resolves true when closed. */
  async requestClosePane(id: PaneId): Promise<boolean> {
    if (!(await this.confirmBusy([id]))) return false;
    if (!this.state.panes[id]) return false;
    this.dispatch(closePane(this.state, id));
    return true;
  }

  async requestCloseTab(tabId: string): Promise<boolean> {
    const tab = this.state.tabs.find((t) => t.id === tabId);
    if (!tab) return false;
    if (!(await this.confirmBusy(leaves(tab.root)))) return false;
    if (!this.state.tabs.some((t) => t.id === tabId)) return false;
    this.dispatch(closeTab(this.state, tabId));
    return true;
  }

  private focusedId(): PaneId {
    const tab = this.state.tabs.find((t) => t.id === this.state.active);
    return tab?.focused ?? "";
  }

  /** `busy: true` asks; `false`, `null` (unknown) or a failed query do not (D21). */
  private async confirmBusy(ids: readonly PaneId[]): Promise<boolean> {
    const busy: string[] = [];
    await Promise.all(
      ids.map(async (id) => {
        const pane = this.state.panes[id];
        const session = this.live.get(id)?.session;
        if (!session || pane?.state !== "running") return;
        const info = await session.info().catch(() => null);
        if (info?.busy === true) busy.push(sanitizeTitle(info.foreground ?? "") ?? "a program");
      }),
    );
    if (busy.length === 0) return true;
    const what = busy.length === 1 ? `"${busy[0]}" is` : `${busy.map((b) => `"${b}"`).join(", ")} are`;
    this.confirming = true;
    try {
      return await this.deps.confirm(`${what} still running. Close anyway?`);
    } finally {
      this.confirming = false;
      this.live.get(this.focusedId())?.view?.focus();
    }
  }

  private dispatch(result: Result): void {
    if (this.disposed) return;
    this.state = result.layout;
    this.apply(result);
  }

  private apply(result: Result): void {
    for (const effect of result.effects) if (effect.type === "openPty") this.prepare(effect.paneId);
    for (const effect of result.effects) if (effect.type === "closePty") this.release(effect.paneId);
    this.render();
    for (const effect of result.effects) this.applyEffect(effect);
  }

  private applyEffect(effect: Effect): void {
    switch (effect.type) {
      case "openPty":
        void this.open(effect.paneId, effect.cwdFrom);
        return;
      case "focusPane":
        this.live.get(effect.paneId)?.view?.focus();
        return;
      case "closeWindow":
        this.deps.closeWindow();
        return;
      case "closePty":
        return; // Released before rendering.
    }
  }

  private render(): void {
    const views = new Map<PaneId, PaneView>();
    for (const [id, live] of this.live) {
      views.set(id, live.view ?? placeholder(live.element));
    }
    this.deps.renderer.render(this.state, views);
  }

  private prepare(id: PaneId): void {
    if (!this.live.has(id)) this.live.set(id, { element: this.deps.createElement(), view: null, session: null, subscriptions: [] });
  }

  /**
   * Autocomplete, key handlers and listeners go first, then the pty: an exit
   * caused by our own `close` never reaches `paneExited`.
   */
  private release(id: PaneId): void {
    const live = this.live.get(id);
    if (!live) return;
    this.live.delete(id);
    for (const d of live.subscriptions.splice(0).reverse()) d.dispose();
    live.session?.close().catch(() => {
      // Already gone.
    });
    live.view?.dispose();
    live.element.remove();
  }

  private async open(id: PaneId, cwdFrom: PaneId | null): Promise<void> {
    const live = this.live.get(id);
    if (!live) return;
    const view = this.deps.createPane(live.element);
    live.view = view;
    // Visibility is applied by the renderer; render again so it sees the view.
    this.render();
    if (this.focusedId() === id) view.focus();

    const cwd = cwdFrom ? await this.cwdOf(cwdFrom) : null;
    let session: PtySession;
    try {
      session = await this.deps.transport.open({ cols: view.cols, rows: view.rows, ...(cwd !== null && { cwd }) });
    } catch (error) {
      if (this.live.get(id) !== live) return;
      const message = error instanceof Error ? error.message : String(error);
      view.write(`[termbay] could not start the shell: ${sanitizeTitle(message) ?? "unknown error"}\r\n${ENTER_HINT}`);
      this.state = updatePane(this.state, id, { state: "exited" });
      view.setExited(() => this.dispatch(closePane(this.state, id)));
      return;
    }
    if (this.live.get(id) !== live || this.disposed) {
      session.close().catch(() => {});
      return;
    }
    live.session = session;
    live.subscriptions.push(
      view.attach(session, this.trigger ? { trigger: this.trigger } : {}),
      session.onExit((exit) => this.exited(id, exit)),
      view.onTitle((oscTitle) => this.patch(id, { oscTitle })),
      view.onCwdLabel((cwdLabel) => this.patch(id, { cwdLabel })),
    );
    if (this.state.panes[id]?.state === "starting") this.patch(id, { state: "running", ptyId: session.id });
    session
      .info()
      .then((info) => this.patch(id, { shell: sanitizeTitle(info.shell) }))
      .catch(() => {
        // The name is only a title fallback.
      });
  }

  /** Directory of a pane's shell process, from the core (D23); never from OSC 7. */
  private async cwdOf(id: PaneId): Promise<string | null> {
    const session = this.live.get(id)?.session;
    if (!session) return null;
    try {
      return (await session.info()).cwd;
    } catch {
      return null;
    }
  }

  private patch(id: PaneId, fields: Parameters<typeof updatePane>[2]): void {
    if (!this.state.panes[id]) return;
    this.state = updatePane(this.state, id, fields);
    this.render();
  }

  private exited(id: PaneId, exit: PtyExit): void {
    if (!this.state.panes[id]) return;
    const result = paneExited(this.state, id, exit.exitCode);
    if (result.effects.length > 0) {
      this.dispatch(result);
      return;
    }
    this.state = result.layout;
    const live = this.live.get(id);
    live?.view?.write(ENTER_HINT);
    live?.view?.setExited(() => this.dispatch(closePane(this.state, id)));
    this.render();
  }

  dispose(): void {
    this.disposed = true;
    for (const id of [...this.live.keys()]) this.release(id);
  }
}

/** Stand-in for a pane whose terminal is created right after the first render. */
function placeholder(element: HTMLElement): PaneView {
  const none = { dispose: () => {} };
  return {
    element,
    cols: 80,
    rows: 24,
    attach: () => none,
    focus: () => {},
    setVisible: () => {},
    onTitle: () => none,
    onCwdLabel: () => none,
    write: () => {},
    setExited: () => {},
    dispose: () => {},
  };
}
