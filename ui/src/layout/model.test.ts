import { describe, expect, it } from "vitest";
import {
  activeTab,
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
  paneRects,
  prevTab,
  setRatio,
  split,
  tabTitle,
  updatePane,
  type Layout,
  type Result,
} from "./model";

const start = (): Layout => createLayout().layout;
/** Runs operations in sequence and returns the final layout. */
const run = (layout: Layout, ...ops: Array<(l: Layout) => Result>): Layout => ops.reduce((l, op) => op(l).layout, layout);
const focused = (l: Layout) => activeTab(l).focused;

describe("createLayout / newTab", () => {
  it("starts with one tab, one pane and opens its pty without cwd", () => {
    const { layout, effects } = createLayout();
    expect(layout.tabs).toHaveLength(1);
    expect(effects).toEqual([{ type: "openPty", paneId: "p1", cwdFrom: null }, { type: "focusPane", paneId: "p1" }]);
  });

  it("creates the tab after the active one, inheriting the focused pane's directory", () => {
    let l = run(start(), newTab, newTab); // tabs: t2, t?, t? ; active = last
    l = goToTab(l, 1).layout;
    const r = newTab(l);
    expect(r.layout.tabs.map((t) => t.id).indexOf(r.layout.active)).toBe(1);
    expect(r.effects[0]).toEqual({ type: "openPty", paneId: focused(r.layout), cwdFrom: "p1" });
  });
});

describe("split and closePane", () => {
  it("splits the focused pane in half and focuses the new one, with cwdFrom", () => {
    const r = split(start(), "right");
    const root = activeTab(r.layout).root;
    expect(root).toMatchObject({ kind: "split", axis: "row", ratio: 0.5, first: { id: "p1" } });
    expect(focused(r.layout)).not.toBe("p1");
    expect(r.effects[0]).toEqual({ type: "openPty", paneId: focused(r.layout), cwdFrom: "p1" });
    expect(activeTab(split(start(), "down").layout).root).toMatchObject({ axis: "column" });
  });

  it("splitting the new pane again gives 50/25/25", () => {
    const l = run(start(), (x) => split(x, "right"), (x) => split(x, "right"));
    expect([...paneRects(activeTab(l).root).values()].map((r) => r.w)).toEqual([0.5, 0.25, 0.25]);
  });

  it("closing a pane gives its space to the sibling and focuses the nearest leaf", () => {
    // p1 | (pA / pB): close pA -> p1 | pB, focus on pB.
    let l = split(start(), "right").layout;
    const a = focused(l);
    l = split(l, "down").layout;
    const b = focused(l);
    l = focusPane(l, a).layout;
    const r = closePane(l, a);
    expect(r.effects).toEqual([{ type: "closePty", paneId: a }, { type: "focusPane", paneId: b }]);
    expect(activeTab(r.layout).root).toMatchObject({ kind: "split", first: { id: "p1" }, second: { kind: "pane", id: b } });
    expect(r.layout.panes[a]).toBeUndefined();
  });

  it("closing the second child focuses the last leaf of the first", () => {
    // (p1 / pA) | pB : close pB -> focus pA (nearest to pB).
    let l = split(start(), "right").layout;
    const b = focused(l);
    l = focusPane(l, "p1").layout;
    l = split(l, "down").layout;
    const a = focused(l);
    l = focusPane(l, b).layout;
    expect(focused(closePane(l, b).layout)).toBe(a);
  });

  it("closing an unfocused pane keeps the focus", () => {
    let l = split(start(), "right").layout;
    const r = closePane(l, "p1");
    expect(r.effects).toEqual([{ type: "closePty", paneId: "p1" }]);
    l = r.layout;
    expect(activeTab(l).root.kind).toBe("pane");
  });

  it("closing the last pane closes the tab, activating the right neighbor (or the left one)", () => {
    let l = run(start(), newTab, newTab);
    const [t1, t2, t3] = l.tabs;
    l = goToTab(l, 2).layout;
    let r = closePane(l, t2!.focused);
    expect(r.layout.active).toBe(t3!.id);
    expect(r.effects).toContainEqual({ type: "focusPane", paneId: t3!.focused });
    r = closePane(r.layout, t3!.focused);
    expect(r.layout.active).toBe(t1!.id);
  });

  it("closing the last tab emits closeWindow", () => {
    const r = closePane(start(), "p1");
    expect(r.effects).toEqual([{ type: "closePty", paneId: "p1" }, { type: "closeWindow" }]);
  });

  it("closeTab closes every pane of the tab", () => {
    let l = run(start(), (x) => split(x, "right"), newTab);
    const first = l.tabs[0]!;
    const r = closeTab(l, first.id);
    expect(r.effects.filter((e) => e.type === "closePty").map((e) => ("paneId" in e ? e.paneId : ""))).toEqual(leaves(first.root));
    expect(r.layout.tabs).toHaveLength(1);
    expect(closeTab(r.layout, r.layout.active).effects.at(-1)).toEqual({ type: "closeWindow" });
  });
});

describe("focus(direction)", () => {
  it("moves inside an L layout: p1 | (pA / pB)", () => {
    let l = split(start(), "right").layout;
    const a = focused(l);
    l = split(l, "down").layout;
    const b = focused(l);
    expect(focused(focus(l, "up").layout)).toBe(a);
    expect(focused(focus(l, "left").layout)).toBe("p1");
    // From p1 to the right, both pA and pB touch it: tie goes to the topmost.
    l = focusPane(l, "p1").layout;
    expect(focused(focus(l, "right").layout)).toBe(a);
    expect(focus(l, "left").effects).toEqual([]);
    expect(focus(l, "up").layout).toBe(l);
    void b;
  });

  it("moves inside a T layout: (pA | pB) / p1", () => {
    let l = split(start(), "down").layout; // p1 / pX
    l = focusPane(l, "p1").layout;
    l = split(l, "right").layout; // (p1 | pB) / pX
    const b = focused(l);
    const x = leaves(activeTab(l).root)[2]!;
    expect(focused(focus(l, "down").layout)).toBe(x);
    expect(focused(focus(l, "left").layout)).toBe("p1");
    l = focusPane(l, x).layout;
    // pX touches both p1 and pB from below: tie goes to the leftmost.
    expect(focused(focus(l, "up").layout)).toBe("p1");
    void b;
  });

  it("uses the ratio for geometry", () => {
    // p1 | pA, then pA split down at 0.5; move the left divider so nothing changes in overlap.
    let l = split(start(), "right").layout;
    const splitId = (activeTab(l).root as { id: string }).id;
    l = setRatio(l, splitId, 0.3).layout;
    expect(paneRects(activeTab(l).root).get("p1")?.w).toBeCloseTo(0.3);
  });
});

describe("tabs navigation", () => {
  it("next/prev are circular and goToTab(9) is the last", () => {
    let l = run(start(), newTab, newTab, newTab);
    const ids = l.tabs.map((t) => t.id);
    expect(nextTab(l).layout.active).toBe(ids[0]);
    l = goToTab(l, 1).layout;
    expect(l.active).toBe(ids[0]);
    expect(prevTab(l).layout.active).toBe(ids[3]);
    expect(goToTab(l, 9).layout.active).toBe(ids[3]);
    expect(goToTab(l, 7).layout).toBe(l);
    expect(goToTab(l, 2).effects).toEqual([{ type: "focusPane", paneId: l.tabs[1]!.focused }]);
  });
});

describe("setRatio", () => {
  it("clamps between 0.1 and 0.9", () => {
    const l = split(start(), "right").layout;
    const id = (activeTab(l).root as { id: string }).id;
    expect(activeTab(setRatio(l, id, 0.01).layout).root).toMatchObject({ ratio: 0.1 });
    expect(activeTab(setRatio(l, id, 2).layout).root).toMatchObject({ ratio: 0.9 });
    expect(activeTab(setRatio(l, id, Number.NaN).layout).root).toMatchObject({ ratio: 0.5 });
  });
});

describe("paneExited (D20)", () => {
  it("code 0 closes the pane; 1 and signal keep it as exited", () => {
    const l = split(start(), "right").layout;
    expect(paneExited(l, "p1", 0).effects[0]).toEqual({ type: "closePty", paneId: "p1" });
    const kept = paneExited(l, "p1", 1);
    expect(kept.effects).toEqual([]);
    expect(kept.layout.panes.p1).toMatchObject({ state: "exited", exitCode: 1 });
    expect(paneExited(l, "p1", null).layout.panes.p1).toMatchObject({ state: "exited", exitCode: null });
  });
});

describe("titles", () => {
  it("prefers the OSC title, then the OSC 7 folder, then the shell", () => {
    let l = start();
    const tab = () => activeTab(l);
    expect(tabTitle(l, tab())).toBe("");
    l = updatePane(l, "p1", { shell: "zsh" });
    expect(tabTitle(l, tab())).toBe("zsh");
    l = updatePane(l, "p1", { cwdLabel: "src" });
    expect(tabTitle(l, tab())).toBe("src");
    l = updatePane(l, "p1", { oscTitle: "vim" });
    expect(tabTitle(l, tab())).toBe("vim");
  });
});
