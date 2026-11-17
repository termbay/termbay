/**
 * Tabs and pane tree (docs/FASE1-PLANO.md, section 6.4). Pure TypeScript: no
 * DOM, no transport. Each operation takes the state and returns the new state
 * plus a list of effects that the layout controller applies.
 */

export type PaneId = string;
/** Split action: new pane to the right (side by side) or below. */
export type Direction = "right" | "down";
export type FocusDirection = "left" | "right" | "up" | "down";

export type Node =
  | { readonly kind: "pane"; readonly id: PaneId }
  | {
      readonly kind: "split";
      readonly id: string;
      /** row = side by side (vertical divider); column = one above the other. */
      readonly axis: "row" | "column";
      /** Fraction of the first child, between MIN_RATIO and MAX_RATIO. */
      readonly ratio: number;
      readonly first: Node;
      readonly second: Node;
    };

export type SplitNode = Extract<Node, { kind: "split" }>;

export interface Tab {
  readonly id: string;
  readonly root: Node;
  readonly focused: PaneId;
}

export interface Pane {
  readonly id: PaneId;
  readonly ptyId: string | null;
  readonly state: "starting" | "running" | "exited";
  readonly exitCode: number | null;
  readonly shell: string | null;
  readonly oscTitle: string | null;
  readonly cwdLabel: string | null;
}

export interface Layout {
  readonly tabs: readonly Tab[];
  readonly active: string;
  readonly panes: Readonly<Record<PaneId, Pane>>;
  /** Counter for new ids. */
  readonly seq: number;
}

export type Effect =
  /** Open a pty for the pane; `cwdFrom` is the pane whose shell directory to inherit (D23). */
  | { readonly type: "openPty"; readonly paneId: PaneId; readonly cwdFrom: PaneId | null }
  | { readonly type: "closePty"; readonly paneId: PaneId }
  | { readonly type: "focusPane"; readonly paneId: PaneId }
  /** The last tab closed (D19). */
  | { readonly type: "closeWindow" };

export interface Result {
  readonly layout: Layout;
  readonly effects: readonly Effect[];
}

export const MIN_RATIO = 0.1;
export const MAX_RATIO = 0.9;

export interface Rect {
  readonly x: number;
  readonly y: number;
  readonly w: number;
  readonly h: number;
}

const EPS = 1e-9;

function newPane(id: PaneId): Pane {
  return { id, ptyId: null, state: "starting", exitCode: null, shell: null, oscTitle: null, cwdLabel: null };
}

const unchanged = (layout: Layout): Result => ({ layout, effects: [] });

/** A window with one tab and one pane, plus the effects to start it. */
export function createLayout(): Result {
  const paneId = "p1";
  const layout: Layout = {
    tabs: [{ id: "t2", root: { kind: "pane", id: paneId }, focused: paneId }],
    active: "t2",
    panes: { [paneId]: newPane(paneId) },
    seq: 2,
  };
  return { layout, effects: [{ type: "openPty", paneId, cwdFrom: null }, { type: "focusPane", paneId }] };
}

export function activeTab(layout: Layout): Tab {
  const tab = layout.tabs.find((t) => t.id === layout.active) ?? layout.tabs[0];
  if (!tab) throw new Error("layout without tabs");
  return tab;
}

export function focusedPane(layout: Layout): PaneId {
  return activeTab(layout).focused;
}

/** Leaves of a tree, in order (left/top first). */
export function leaves(node: Node): PaneId[] {
  return node.kind === "pane" ? [node.id] : [...leaves(node.first), ...leaves(node.second)];
}

export function tabOf(layout: Layout, paneId: PaneId): Tab | undefined {
  return layout.tabs.find((t) => leaves(t.root).includes(paneId));
}

function replaceTab(layout: Layout, tab: Tab): Layout {
  return { ...layout, tabs: layout.tabs.map((t) => (t.id === tab.id ? tab : t)) };
}

/** Updates fields of one pane (titles, state, pty id). Unknown pane: no change. */
export function updatePane(layout: Layout, id: PaneId, patch: Partial<Omit<Pane, "id">>): Layout {
  const pane = layout.panes[id];
  if (!pane) return layout;
  return { ...layout, panes: { ...layout.panes, [id]: { ...pane, ...patch } } };
}

export function newTab(layout: Layout): Result {
  const from = focusedPane(layout);
  const paneId = `p${layout.seq + 1}`;
  const tabId = `t${layout.seq + 2}`;
  const index = layout.tabs.findIndex((t) => t.id === layout.active);
  const tabs = [...layout.tabs];
  tabs.splice(index + 1, 0, { id: tabId, root: { kind: "pane", id: paneId }, focused: paneId });
  return {
    layout: { tabs, active: tabId, panes: { ...layout.panes, [paneId]: newPane(paneId) }, seq: layout.seq + 2 },
    effects: [
      { type: "openPty", paneId, cwdFrom: from },
      { type: "focusPane", paneId },
    ],
  };
}

function mapNode(node: Node, fn: (n: Node) => Node | null): Node {
  const replaced = fn(node);
  if (replaced) return replaced;
  if (node.kind === "pane") return node;
  return { ...node, first: mapNode(node.first, fn), second: mapNode(node.second, fn) };
}

export function split(layout: Layout, direction: Direction): Result {
  const tab = activeTab(layout);
  const target = tab.focused;
  const paneId = `p${layout.seq + 1}`;
  const splitId = `s${layout.seq + 2}`;
  const root = mapNode(tab.root, (n) =>
    n.kind === "pane" && n.id === target
      ? {
          kind: "split",
          id: splitId,
          axis: direction === "right" ? "row" : "column",
          ratio: 0.5,
          first: n,
          second: { kind: "pane", id: paneId },
        }
      : null,
  );
  const next = replaceTab(
    { ...layout, panes: { ...layout.panes, [paneId]: newPane(paneId) }, seq: layout.seq + 2 },
    { ...tab, root, focused: paneId },
  );
  return {
    layout: next,
    effects: [
      { type: "openPty", paneId, cwdFrom: target },
      { type: "focusPane", paneId },
    ],
  };
}

function firstLeaf(node: Node): PaneId {
  return node.kind === "pane" ? node.id : firstLeaf(node.first);
}

function lastLeaf(node: Node): PaneId {
  return node.kind === "pane" ? node.id : lastLeaf(node.second);
}

/**
 * Removes a leaf: its parent split is replaced by the sibling. Returns the new
 * tree and the leaf nearest to the removed one (inside the sibling).
 */
function removeLeaf(node: Node, id: PaneId): { root: Node; nearest: PaneId } | null {
  if (node.kind === "pane") return null;
  if (node.first.kind === "pane" && node.first.id === id) return { root: node.second, nearest: firstLeaf(node.second) };
  if (node.second.kind === "pane" && node.second.id === id) return { root: node.first, nearest: lastLeaf(node.first) };
  const inFirst = removeLeaf(node.first, id);
  if (inFirst) return { root: { ...node, first: inFirst.root }, nearest: inFirst.nearest };
  const inSecond = removeLeaf(node.second, id);
  if (inSecond) return { root: { ...node, second: inSecond.root }, nearest: inSecond.nearest };
  return null;
}

function withoutPanes(layout: Layout, ids: readonly PaneId[]): Layout {
  const panes = { ...layout.panes };
  for (const id of ids) delete panes[id];
  return { ...layout, panes };
}

/** Removes a whole tab; the right neighbor (or the left one) becomes active. */
function dropTab(layout: Layout, tabId: string, effects: Effect[]): Result {
  const index = layout.tabs.findIndex((t) => t.id === tabId);
  const tabs = layout.tabs.filter((t) => t.id !== tabId);
  if (tabs.length === 0) {
    return { layout: { ...layout, tabs: [], active: "" }, effects: [...effects, { type: "closeWindow" }] };
  }
  if (layout.active !== tabId) return { layout: { ...layout, tabs }, effects };
  const next = tabs[Math.min(index, tabs.length - 1)] as Tab;
  return {
    layout: { ...layout, tabs, active: next.id },
    effects: [...effects, { type: "focusPane", paneId: next.focused }],
  };
}

export function closePane(layout: Layout, id: PaneId): Result {
  const tab = tabOf(layout, id);
  if (!tab) return unchanged(layout);
  const effects: Effect[] = [{ type: "closePty", paneId: id }];
  const base = withoutPanes(layout, [id]);
  const removed = removeLeaf(tab.root, id);
  if (!removed) return dropTab(base, tab.id, effects);

  const focused = tab.focused === id ? removed.nearest : tab.focused;
  if (focused !== tab.focused && layout.active === tab.id) effects.push({ type: "focusPane", paneId: focused });
  return { layout: replaceTab(base, { ...tab, root: removed.root, focused }), effects };
}

/** Closes every pane of a tab. Confirmation (D21) is the controller's job. */
export function closeTab(layout: Layout, tabId: string): Result {
  const tab = layout.tabs.find((t) => t.id === tabId);
  if (!tab) return unchanged(layout);
  const ids = leaves(tab.root);
  return dropTab(
    withoutPanes(layout, ids),
    tabId,
    ids.map((paneId) => ({ type: "closePty", paneId })),
  );
}

/** Normalized rectangles (0..1) of every leaf of a tree. */
export function paneRects(node: Node, rect: Rect = { x: 0, y: 0, w: 1, h: 1 }): Map<PaneId, Rect> {
  const out = new Map<PaneId, Rect>();
  const walk = (n: Node, r: Rect) => {
    if (n.kind === "pane") {
      out.set(n.id, r);
      return;
    }
    if (n.axis === "row") {
      const w1 = r.w * n.ratio;
      walk(n.first, { ...r, w: w1 });
      walk(n.second, { ...r, x: r.x + w1, w: r.w - w1 });
    } else {
      const h1 = r.h * n.ratio;
      walk(n.first, { ...r, h: h1 });
      walk(n.second, { ...r, y: r.y + h1, h: r.h - h1 });
    }
  };
  walk(node, rect);
  return out;
}

const overlaps = (a0: number, a1: number, b0: number, b1: number) => a0 < b1 - EPS && b0 < a1 - EPS;

/**
 * Moves the focus by geometry: the nearest pane in that direction that
 * overlaps the current one on the other axis. Ties go to the topmost, then
 * leftmost. No neighbor: no change.
 */
export function focus(layout: Layout, direction: FocusDirection): Result {
  const tab = activeTab(layout);
  const rects = paneRects(tab.root);
  const cur = rects.get(tab.focused);
  if (!cur) return unchanged(layout);
  let best: { id: PaneId; dist: number; r: Rect } | null = null;
  for (const [id, r] of rects) {
    if (id === tab.focused) continue;
    let dist: number;
    if (direction === "left" || direction === "right") {
      if (!overlaps(cur.y, cur.y + cur.h, r.y, r.y + r.h)) continue;
      dist = direction === "left" ? cur.x - (r.x + r.w) : r.x - (cur.x + cur.w);
    } else {
      if (!overlaps(cur.x, cur.x + cur.w, r.x, r.x + r.w)) continue;
      dist = direction === "up" ? cur.y - (r.y + r.h) : r.y - (cur.y + cur.h);
    }
    if (dist < -EPS) continue;
    const better =
      !best ||
      dist < best.dist - EPS ||
      (Math.abs(dist - best.dist) <= EPS && (r.y < best.r.y - EPS || (Math.abs(r.y - best.r.y) <= EPS && r.x < best.r.x)));
    if (better) best = { id, dist, r };
  }
  if (!best) return unchanged(layout);
  return { layout: replaceTab(layout, { ...tab, focused: best.id }), effects: [{ type: "focusPane", paneId: best.id }] };
}

/** Click on a pane: its tab becomes active and the pane gets the focus. */
export function focusPane(layout: Layout, id: PaneId): Result {
  const tab = tabOf(layout, id);
  if (!tab) return unchanged(layout);
  return {
    layout: replaceTab({ ...layout, active: tab.id }, { ...tab, focused: id }),
    effects: [{ type: "focusPane", paneId: id }],
  };
}

function activate(layout: Layout, index: number): Result {
  const tab = layout.tabs[index];
  if (!tab || tab.id === layout.active) return unchanged(layout);
  return { layout: { ...layout, active: tab.id }, effects: [{ type: "focusPane", paneId: tab.focused }] };
}

export function nextTab(layout: Layout): Result {
  const n = layout.tabs.length;
  return activate(layout, (layout.tabs.findIndex((t) => t.id === layout.active) + 1) % n);
}

export function prevTab(layout: Layout): Result {
  const n = layout.tabs.length;
  return activate(layout, (layout.tabs.findIndex((t) => t.id === layout.active) - 1 + n) % n);
}

/** 1 to 8 by position; 9 is always the last tab, as in browsers. */
export function goToTab(layout: Layout, n: number): Result {
  if (n === 9) return activate(layout, layout.tabs.length - 1);
  if (!Number.isInteger(n) || n < 1 || n > 8) return unchanged(layout);
  return activate(layout, n - 1);
}

export const clampRatio = (r: number): number =>
  Number.isFinite(r) ? Math.min(MAX_RATIO, Math.max(MIN_RATIO, r)) : 0.5;

export function setRatio(layout: Layout, splitId: string, ratio: number): Result {
  const r = clampRatio(ratio);
  const tabs = layout.tabs.map((tab) => ({
    ...tab,
    root: mapNode(tab.root, (n) => (n.kind === "split" && n.id === splitId ? { ...n, ratio: r } : null)),
  }));
  return { layout: { ...layout, tabs }, effects: [] };
}

/**
 * The shell of a pane ended (D20): code 0 closes the pane; any other code, or
 * death by signal (null), keeps it with the notice until Enter or Cmd+W.
 */
export function paneExited(layout: Layout, id: PaneId, exitCode: number | null): Result {
  if (!layout.panes[id]) return unchanged(layout);
  if (exitCode === 0) return closePane(layout, id);
  return unchanged(updatePane(layout, id, { state: "exited", exitCode }));
}

/** Tab title: the focused pane's OSC title, else its OSC 7 folder, else the shell name. */
export function paneTitle(pane: Pane | undefined): string {
  return pane?.oscTitle ?? pane?.cwdLabel ?? pane?.shell ?? "";
}

export function tabTitle(layout: Layout, tab: Tab): string {
  return paneTitle(layout.panes[tab.focused]);
}
