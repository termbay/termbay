// The office panel (task B2 in docs/FASE2-PLANO.md, decision E9): beside the
// terminals, with a draggable divider and a close button. It listens to the
// office from startup to notice the first agent, but loads its content (the
// scene) only on the first opening, and hides it when closed.

import type { OfficeEvent, OfficeListener, OfficeSnapshot } from "../transport/office";
import type { OfficeContent, OfficeContentLoader } from "./content";
import { DEFAULT_PANEL_WIDTH, PanelModel, hasAgents } from "./panel-model";

/** Changes kept while the content loads or before it is first opened. */
export const MAX_HELD_EVENTS = 500;

/** Pixels the divider moves per arrow key. */
export const KEY_STEP = 16;

/** Key of the remembered width in the local storage. */
export const WIDTH_KEY = "termbay.office.width";

/** Where the width the user chose is remembered. */
export interface WidthStore {
  load(): number | undefined;
  save(width: number): void;
}

/**
 * The width in the page's local storage. Storage can be missing or refuse
 * access (private modes, policies): then nothing is remembered, silently.
 */
export function localWidthStore(storage: () => Storage | null | undefined): WidthStore {
  return {
    load() {
      try {
        const value = Number(storage()?.getItem(WIDTH_KEY) ?? Number.NaN);
        return Number.isFinite(value) && value > 0 ? value : undefined;
      } catch {
        return undefined;
      }
    },
    save(width) {
      try {
        storage()?.setItem(WIDTH_KEY, String(Math.round(width)));
      } catch {
        // Not remembered; the panel works the same.
      }
    },
  };
}

export interface OfficePanelOptions {
  /** `#app`: holds the terminals and the panel side by side. */
  readonly root: HTMLElement;
  /** `#office-panel`, hidden until it opens. */
  readonly panel: HTMLElement;
  readonly model: PanelModel;
  /** Builds the content on the first opening (import() inside). */
  readonly load: OfficeContentLoader;
  readonly store?: WidthStore | undefined;
  /**
   * A fresh subscription for the content, used when more changes came
   * before the first opening than the panel keeps (MAX_HELD_EVENTS).
   */
  readonly resubscribe?: ((listener: OfficeListener) => Promise<void>) | undefined;
  /** Gives the keyboard back to the terminal when the panel closes. */
  readonly focusTerminal?: (() => void) | undefined;
  /** Width of the window, in CSS pixels. */
  readonly viewport?: (() => number) | undefined;
}

/** Changes seen before the content exists. */
interface Held {
  snapshot: OfficeSnapshot | null;
  events: OfficeEvent[];
  /** More than MAX_HELD_EVENTS came: the content needs a fresh picture. */
  overflow: boolean;
}

export class OfficePanel implements OfficeListener {
  private readonly divider: HTMLElement;
  private readonly host: HTMLElement;
  private readonly closeButton: HTMLButtonElement;
  private readonly viewport: () => number;
  private readonly cleanup: Array<() => void> = [];
  private content: OfficeContent | null = null;
  private loading: Promise<void> | null = null;
  private held: Held | null = { snapshot: null, events: [], overflow: false };
  /** False when the content has its own subscription. */
  private forward = true;
  private destroyed = false;

  constructor(private readonly options: OfficePanelOptions) {
    const { root, panel } = options;
    const document = root.ownerDocument;
    const view = document.defaultView;
    this.viewport = options.viewport ?? (() => view?.innerWidth ?? 0);

    this.divider = document.createElement("div");
    this.divider.className = "office-divider";
    this.divider.tabIndex = 0;
    this.divider.setAttribute("role", "separator");
    this.divider.setAttribute("aria-orientation", "vertical");
    this.divider.setAttribute("aria-label", "Resize the office");
    this.divider.hidden = true;
    panel.before(this.divider);

    const header = document.createElement("div");
    header.className = "office-header";
    const title = document.createElement("span");
    title.className = "office-title";
    title.textContent = "Office";
    this.closeButton = document.createElement("button");
    this.closeButton.type = "button";
    this.closeButton.className = "office-close";
    this.closeButton.setAttribute("aria-label", "Close the office");
    this.closeButton.title = "Close the office";
    this.closeButton.textContent = "×";
    header.append(title, this.closeButton);

    this.host = document.createElement("div");
    this.host.className = "office-content";
    panel.replaceChildren(header, this.host);
    panel.hidden = true;

    this.listen(this.closeButton, "click", () => this.close());
    this.listen(this.divider, "pointerdown", (ev) => this.startDrag(ev as PointerEvent));
    this.listen(this.divider, "keydown", (ev) => this.onKey(ev as KeyboardEvent));
    this.listen(this.divider, "dblclick", () => this.setWidth(DEFAULT_PANEL_WIDTH, true));
    if (view) this.listen(view, "resize", () => this.applyWidth());
  }

  get open(): boolean {
    return this.options.model.open;
  }

  /** The element the content draws in. */
  get contentHost(): HTMLElement {
    return this.host;
  }

  /** The divider element (tests and e2e). */
  get dividerElement(): HTMLElement {
    return this.divider;
  }

  snapshot(snapshot: OfficeSnapshot): void {
    if (this.held) {
      this.held.snapshot = snapshot;
      this.held.events = [];
      this.held.overflow = false;
    } else if (this.forward) this.content?.snapshot(snapshot);
    if (hasAgents(snapshot) && this.options.model.agentSeen()) this.render();
  }

  event(event: OfficeEvent): void {
    if (this.held) {
      if (this.held.events.length >= MAX_HELD_EVENTS) {
        this.held.events = [];
        this.held.overflow = true;
      }
      if (!this.held.overflow) this.held.events.push(event);
    } else if (this.forward) this.content?.event(event);
    if (this.options.model.agentSeen()) this.render();
  }

  /** The shortcut (toggleOffice). */
  toggle(): void {
    const focusInside = this.options.panel.contains(this.options.root.ownerDocument.activeElement);
    this.options.model.toggle();
    this.render();
    if (!this.open && focusInside) this.options.focusTerminal?.();
  }

  /** The close button. */
  close(): void {
    if (!this.open) return;
    this.options.model.close();
    this.render();
    this.options.focusTerminal?.();
  }

  /** Resolves once the content is loaded (or failed to load). */
  async ready(): Promise<void> {
    await this.loading;
  }

  destroy(): void {
    this.destroyed = true;
    for (const undo of this.cleanup.splice(0)) undo();
    this.content?.destroy();
    this.content = null;
    this.divider.remove();
  }

  private render(): void {
    const open = this.open;
    this.options.panel.hidden = !open;
    this.divider.hidden = !open;
    if (open) {
      this.applyWidth();
      this.ensureContent();
    }
    this.content?.setVisible(open);
  }

  private ensureContent(): void {
    if (this.content || this.loading || this.destroyed) return;
    this.loading = this.options
      .load(this.host)
      .then((content) => this.attach(content))
      .catch((error: unknown) => {
        console.error("[termbay] could not load the office", error);
        this.loading = null;
      });
  }

  private attach(content: OfficeContent): void {
    if (this.destroyed) {
      content.destroy();
      return;
    }
    this.content = content;
    const held = this.held;
    this.held = null;
    if (held?.overflow && this.options.resubscribe) {
      this.forward = false;
      this.options.resubscribe(content).catch((error: unknown) =>
        console.error("[termbay] could not subscribe the office", error),
      );
    } else if (held) {
      if (held.snapshot) content.snapshot(held.snapshot);
      for (const event of held.events) content.event(event);
    }
    content.setVisible(this.open);
  }

  private applyWidth(): void {
    if (!this.open) return;
    const width = this.options.model.widthFor(this.viewport());
    this.options.panel.style.width = `${width}px`;
    this.divider.setAttribute("aria-valuenow", String(width));
  }

  private setWidth(width: number, save: boolean): void {
    this.options.model.resize(width, this.viewport());
    this.applyWidth();
    if (save) this.options.store?.save(this.options.model.width);
  }

  private startDrag(ev: PointerEvent): void {
    if (ev.button !== 0) return;
    ev.preventDefault();
    const { root } = this.options;
    const right = root.getBoundingClientRect().right;
    const target = this.divider;
    target.setPointerCapture?.(ev.pointerId);
    root.classList.add("office-resizing");
    const move = (e: Event) => this.setWidth(right - (e as PointerEvent).clientX, false);
    const end = () => {
      target.removeEventListener("pointermove", move);
      target.removeEventListener("pointerup", end);
      target.removeEventListener("pointercancel", end);
      root.classList.remove("office-resizing");
      this.options.store?.save(this.options.model.width);
    };
    target.addEventListener("pointermove", move);
    target.addEventListener("pointerup", end);
    target.addEventListener("pointercancel", end);
  }

  private onKey(ev: KeyboardEvent): void {
    // The panel is on the right: moving the divider left makes it wider.
    const step = ev.key === "ArrowLeft" ? KEY_STEP : ev.key === "ArrowRight" ? -KEY_STEP : 0;
    if (step === 0) return;
    ev.preventDefault();
    this.setWidth(this.options.model.widthFor(this.viewport()) + step, true);
  }

  private listen(target: EventTarget, type: string, handler: (ev: Event) => void): void {
    target.addEventListener(type, handler);
    this.cleanup.push(() => target.removeEventListener(type, handler));
  }
}
