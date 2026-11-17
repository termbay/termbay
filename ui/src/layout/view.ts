import type { Disposable } from "../transport/types";
import { MAX_RATIO, MIN_RATIO, leaves, tabTitle, type Layout, type Node, type PaneId } from "./model";

/** Minimum pane size in pixels, enforced while dragging a divider. */
export const MIN_PANE_WIDTH = 80;
export const MIN_PANE_HEIGHT = 48;

export interface LayoutViewEvents {
  newTab(): void;
  activateTab(tabId: string): void;
  closeTab(tabId: string): void;
  focusPane(paneId: PaneId): void;
  setRatio(splitId: string, ratio: number): void;
}

/** The parts of a pane the DOM layout handles. */
export interface PlacedPane {
  readonly element: HTMLElement;
  setVisible(visible: boolean): void;
}

/**
 * Ratio for a divider dragged to `pointer` inside a split that starts at
 * `start` and is `size` pixels long, keeping both sides at least `minPx`.
 */
export function ratioFromPointer(pointer: number, start: number, size: number, minPx: number): number {
  if (size <= 0) return 0.5;
  const low = Math.max(MIN_RATIO, minPx / size);
  const high = Math.min(MAX_RATIO, 1 - minPx / size);
  const r = (pointer - start) / size;
  if (low > high) return 0.5;
  return Math.min(high, Math.max(low, r));
}

/** Tree shape without ratios: when it is unchanged, the DOM only updates sizes. */
function shapeOf(node: Node): string {
  return node.kind === "pane" ? node.id : `${node.id}:${node.axis}(${shapeOf(node.first)},${shapeOf(node.second)})`;
}

interface TabPanel {
  readonly element: HTMLElement;
  shape: string;
  readonly splits: Map<string, { first: HTMLElement; second: HTMLElement }>;
}

/**
 * DOM of the tabs (docs/FASE1-PLANO.md, 6.4) inside `#terminals`: a vertical
 * tab list on the left (collapsible), and one panel per tab, of which only the active one is visible. Pane
 * elements are moved, never recreated. Titles only go through `textContent`.
 */
export class LayoutView implements Disposable {
  readonly tabbar: HTMLElement;
  readonly panelsRoot: HTMLElement;
  private readonly panels = new Map<string, TabPanel>();
  private readonly visible = new Map<PaneId, boolean>();
  private readonly doc: Document;
  private collapsed = false;

  constructor(
    private readonly root: HTMLElement,
    private readonly events: LayoutViewEvents,
  ) {
    this.doc = root.ownerDocument;
    this.tabbar = this.doc.createElement("div");
    this.tabbar.className = "tabbar";
    this.tabbar.setAttribute("role", "tablist");
    this.tabbar.setAttribute("aria-orientation", "vertical");
    this.panelsRoot = this.doc.createElement("div");
    this.panelsRoot.className = "tab-panels";
    root.append(this.tabbar, this.panelsRoot);
  }

  render(layout: Layout, panes: ReadonlyMap<PaneId, PlacedPane>): void {
    this.renderTabbar(layout);

    for (const [id, panel] of this.panels) {
      if (!layout.tabs.some((t) => t.id === id)) {
        panel.element.remove();
        this.panels.delete(id);
      }
    }

    for (const tab of layout.tabs) {
      let panel = this.panels.get(tab.id);
      if (!panel) {
        const element = this.doc.createElement("div");
        element.className = "tab-panel";
        element.setAttribute("role", "tabpanel");
        this.panelsRoot.append(element);
        panel = { element, shape: "", splits: new Map() };
        this.panels.set(tab.id, panel);
      }
      const shape = shapeOf(tab.root);
      if (shape !== panel.shape) {
        panel.splits.clear();
        panel.element.replaceChildren(this.build(tab.root, panes, panel));
        panel.shape = shape;
      }
      this.applyRatios(tab.root, panel);
      const active = tab.id === layout.active;
      panel.element.hidden = !active;

      for (const id of leaves(tab.root)) {
        const pane = panes.get(id);
        if (!pane) continue;
        pane.element.classList.toggle("focused", id === tab.focused);
        if (this.visible.get(id) !== active) {
          this.visible.set(id, active);
          pane.setVisible(active);
        }
      }
    }
    for (const id of [...this.visible.keys()]) if (!(id in layout.panes)) this.visible.delete(id);
  }

  get tabsCollapsed(): boolean {
    return this.collapsed;
  }

  /** Hides or shows the tab list; the panes refit through their ResizeObserver. */
  toggleTabs(): void {
    this.collapsed = !this.collapsed;
    this.root.classList.toggle("tabs-collapsed", this.collapsed);
    this.tabbar.hidden = this.collapsed;
  }

  private renderTabbar(layout: Layout): void {
    const buttons: HTMLElement[] = layout.tabs.map((tab) => {
      const item = this.doc.createElement("div");
      item.className = "tab";
      item.setAttribute("role", "tab");
      item.setAttribute("aria-selected", String(tab.id === layout.active));
      item.dataset.tabId = tab.id;
      const text = this.doc.createElement("div");
      text.className = "tab-text";
      const label = this.doc.createElement("span");
      label.className = "tab-title";
      label.textContent = tabTitle(layout, tab) || "terminal";
      // Second line: folder from OSC 7, display only (never used to open a pty).
      const folder = this.doc.createElement("span");
      folder.className = "tab-cwd";
      folder.textContent = layout.panes[tab.focused]?.cwdLabel ?? "";
      text.append(label, folder);
      const close = this.doc.createElement("button");
      close.className = "tab-close";
      close.type = "button";
      close.textContent = "×";
      close.setAttribute("aria-label", "Close tab");
      close.addEventListener("click", (ev) => {
        ev.stopPropagation();
        this.events.closeTab(tab.id);
      });
      item.addEventListener("mousedown", (ev) => {
        if ((ev.target as HTMLElement | null)?.closest?.(".tab-close")) return;
        ev.preventDefault(); // keep the terminal focused
        this.events.activateTab(tab.id);
      });
      item.append(text, close);
      return item;
    });
    const add = this.doc.createElement("button");
    add.className = "tab-new";
    add.type = "button";
    add.textContent = "+ New tab";
    add.setAttribute("aria-label", "New tab");
    add.addEventListener("click", () => this.events.newTab());
    this.tabbar.replaceChildren(add, ...buttons);
  }

  private build(node: Node, panes: ReadonlyMap<PaneId, PlacedPane>, panel: TabPanel): HTMLElement {
    if (node.kind === "pane") {
      const element = panes.get(node.id)?.element ?? this.doc.createElement("div");
      element.classList.add("pane");
      if (!element.dataset.paneId) {
        element.dataset.paneId = node.id;
        element.addEventListener("mousedown", () => this.events.focusPane(node.id));
      }
      return element;
    }
    const box = this.doc.createElement("div");
    box.className = `split split-${node.axis}`;
    const first = this.build(node.first, panes, panel);
    const second = this.build(node.second, panes, panel);
    const divider = this.doc.createElement("div");
    divider.className = "divider";
    divider.setAttribute("role", "separator");
    divider.setAttribute("aria-orientation", node.axis === "row" ? "vertical" : "horizontal");
    this.makeDraggable(divider, box, node.id, node.axis);
    box.append(first, divider, second);
    panel.splits.set(node.id, { first, second });
    return box;
  }

  private applyRatios(node: Node, panel: TabPanel): void {
    if (node.kind === "pane") return;
    const els = panel.splits.get(node.id);
    if (els) {
      els.first.style.flex = `${node.ratio} 1 0`;
      els.second.style.flex = `${1 - node.ratio} 1 0`;
    }
    this.applyRatios(node.first, panel);
    this.applyRatios(node.second, panel);
  }

  private makeDraggable(divider: HTMLElement, box: HTMLElement, splitId: string, axis: "row" | "column"): void {
    divider.addEventListener("pointerdown", (down) => {
      down.preventDefault();
      divider.setPointerCapture?.(down.pointerId);
      const move = (ev: PointerEvent) => {
        const rect = box.getBoundingClientRect();
        const ratio =
          axis === "row"
            ? ratioFromPointer(ev.clientX, rect.left, rect.width, MIN_PANE_WIDTH)
            : ratioFromPointer(ev.clientY, rect.top, rect.height, MIN_PANE_HEIGHT);
        this.events.setRatio(splitId, ratio);
      };
      const up = () => {
        divider.removeEventListener("pointermove", move);
        divider.removeEventListener("pointerup", up);
        divider.removeEventListener("pointercancel", up);
      };
      divider.addEventListener("pointermove", move);
      divider.addEventListener("pointerup", up);
      divider.addEventListener("pointercancel", up);
    });
  }

  dispose(): void {
    this.root.classList.remove("tabs-collapsed");
    this.tabbar.remove();
    this.panelsRoot.remove();
    this.panels.clear();
    this.visible.clear();
  }
}
