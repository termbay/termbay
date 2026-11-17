// @vitest-environment happy-dom
import { describe, expect, it, vi } from "vitest";
import { Emitter } from "../transport/emitter";
import { EchoPtySession, EchoPtyTransport } from "../transport/echo";
import type { Disposable, PtyInfo, PtyOpenOptions, PtySession } from "../transport/types";
import { LayoutController } from "./controller";
import { activeTab, leaves } from "./model";
import type { PaneView } from "./pane";
import { LayoutView } from "./view";

class FakePane implements PaneView {
  readonly title = new Emitter<string | null>();
  readonly cwd = new Emitter<string | null>();
  readonly written: string[] = [];
  readonly attached: Array<{ trigger?: string }> = [];
  attachDisposed = 0;
  focused = 0;
  visible: boolean[] = [];
  exitedEnter: (() => void) | null = null;
  disposed = false;
  readonly cols = 80;
  readonly rows = 24;
  constructor(readonly element: HTMLElement) {}
  attach(_: PtySession, options: { trigger?: string }): Disposable {
    this.attached.push(options);
    return { dispose: () => this.attachDisposed++ };
  }
  focus() {
    this.focused++;
  }
  setVisible(v: boolean) {
    this.visible.push(v);
  }
  onTitle(l: (t: string | null) => void) {
    return this.title.on(l);
  }
  onCwdLabel(l: (t: string | null) => void) {
    return this.cwd.on(l);
  }
  write(text: string) {
    this.written.push(text);
  }
  setExited(onEnter: () => void) {
    this.exitedEnter = onEnter;
  }
  dispose() {
    this.disposed = true;
  }
}

class FakeTransport extends EchoPtyTransport {
  readonly opened: PtyOpenOptions[] = [];
  readonly sessions: EchoPtySession[] = [];
  infos = new Map<string, PtyInfo>();
  failNext: string | null = null;
  override async open(options: PtyOpenOptions): Promise<EchoPtySession> {
    this.opened.push(options);
    if (this.failNext) {
      const message = this.failNext;
      this.failNext = null;
      throw new Error(message);
    }
    const session = await super.open(options);
    const infos = this.infos;
    session.info = async () => infos.get(session.id) ?? { shell: "zsh", cwd: null, busy: false, foreground: null };
    this.sessions.push(session);
    return session;
  }
}

const flush = async () => {
  for (let i = 0; i < 5; i++) await Promise.resolve();
  await new Promise((r) => setTimeout(r, 0));
};

async function setup(answer = true) {
  document.body.innerHTML = '<main id="terminals"></main><aside id="office-panel" hidden></aside>';
  const root = document.getElementById("terminals") as HTMLElement;
  const transport = new FakeTransport();
  const panes: FakePane[] = [];
  const confirm = vi.fn(async () => answer);
  const closeWindow = vi.fn();
  let controller!: LayoutController;
  const view = new LayoutView(root, {
    newTab: () => controller.run({ type: "newTab" }),
    activateTab: (id) => controller.activateTab(id),
    closeTab: (id) => void controller.requestCloseTab(id),
    focusPane: (id) => controller.focusPane(id),
    setRatio: (id, r) => controller.setRatio(id, r),
  });
  controller = new LayoutController({
    transport,
    createElement: () => document.createElement("div"),
    createPane: (el) => {
      const p = new FakePane(el);
      panes.push(p);
      return p;
    },
    renderer: view,
    confirm,
    closeWindow,
  });
  await flush();
  return { root, transport, panes, confirm, closeWindow, controller, view };
}

describe("LayoutController: opening panes (D23)", () => {
  it("split asks info() of the focused pane and passes its cwd; null cwd opens without cwd", async () => {
    const t = await setup();
    expect(t.transport.opened[0]).toEqual({ cols: 80, rows: 24 });
    t.transport.infos.set(t.transport.sessions[0]!.id, { shell: "zsh", cwd: "/work/repo", busy: false, foreground: null });
    const info = vi.spyOn(t.transport.sessions[0]!, "info");
    t.controller.run({ type: "split", direction: "right" });
    await flush();
    expect(info).toHaveBeenCalled();
    expect(t.transport.opened[1]).toEqual({ cols: 80, rows: 24, cwd: "/work/repo" });
    t.controller.run({ type: "newTab" });
    await flush();
    expect(t.transport.opened[2]).not.toHaveProperty("cwd");
  });

  it("a forged OSC 7 changes the label, never the cwd of the next open", async () => {
    const t = await setup();
    t.panes[0]!.cwd.emit("evil-repo");
    expect(t.controller.layout.panes.p1?.cwdLabel).toBe("evil-repo");
    expect(t.root.querySelector(".tab-title")?.textContent).toBe("evil-repo");
    expect(t.root.querySelector(".tab-cwd")?.textContent).toBe("evil-repo");
    t.controller.run({ type: "split", direction: "down" });
    await flush();
    expect(t.transport.opened[1]).not.toHaveProperty("cwd");
  });

  it("titles come from OSC, then OSC 7, then the shell, and enter the DOM as text", async () => {
    const t = await setup();
    expect(t.root.querySelector(".tab-title")?.textContent).toBe("zsh");
    t.panes[0]!.title.emit("<img src=x onerror=alert(1)>");
    const label = t.root.querySelector(".tab-title");
    expect(label?.textContent).toBe("<img src=x onerror=alert(1)>");
    expect(label?.querySelector("img")).toBeNull();
  });

  it("the pty limit error stays readable in the pane and Enter closes it", async () => {
    const t = await setup();
    t.transport.failNext = "too many terminals are open (limit 64)";
    t.controller.run({ type: "newTab" });
    await flush();
    expect(t.panes[1]!.written.join("")).toContain("too many terminals are open (limit 64)");
    t.panes[1]!.exitedEnter?.();
    expect(t.controller.layout.tabs).toHaveLength(1);
  });

  it("passes the configured trigger to panes opened afterwards", async () => {
    const t = await setup();
    t.controller.setTrigger("ctrl+space");
    t.controller.run({ type: "newTab" });
    await flush();
    expect(t.panes[0]!.attached[0]).toEqual({});
    expect(t.panes[1]!.attached[0]).toEqual({ trigger: "ctrl+space" });
  });
});

describe("LayoutController: shell exit (D20) and closing (D19, D21)", () => {
  it("exit code 0 closes the pane; 1 keeps it with the notice until Enter", async () => {
    const t = await setup();
    t.controller.run({ type: "split", direction: "right" });
    await flush();
    const [first, second] = t.transport.sessions;
    // Exit 1 on the second pane.
    (second as unknown as { exit: Emitter<{ exitCode: number }> }).exit.emit({ exitCode: 1 });
    expect(leaves(activeTab(t.controller.layout).root)).toHaveLength(2);
    expect(t.panes[1]!.written.join("")).toContain("Press Enter to close the pane.");
    t.panes[1]!.exitedEnter?.();
    expect(leaves(activeTab(t.controller.layout).root)).toEqual(["p1"]);
    // Exit 0 on the last pane closes the window.
    (first as unknown as { exit: Emitter<{ exitCode: number }> }).exit.emit({ exitCode: 0 });
    expect(t.closeWindow).toHaveBeenCalledOnce();
  });

  it("closing disposes autocomplete and listeners before pty close, without a fake exit", async () => {
    const t = await setup();
    t.controller.run({ type: "split", direction: "right" });
    await flush();
    const session = t.transport.sessions[1]!;
    const order: string[] = [];
    const pane = t.panes[1]!;
    const origClose = session.close.bind(session);
    session.close = async () => {
      order.push(`close:attach=${pane.attachDisposed}`);
      return origClose();
    };
    await t.controller.requestClosePane(activeTab(t.controller.layout).focused);
    expect(order).toEqual(["close:attach=1"]);
    expect(pane.disposed).toBe(true);
    // The echo close emits exitCode 0, which must not close another pane.
    expect(t.controller.layout.tabs).toHaveLength(1);
    expect(leaves(activeTab(t.controller.layout).root)).toEqual(["p1"]);
  });

  it("busy: true asks with the program name; cancel keeps the pane", async () => {
    const t = await setup(false);
    t.transport.infos.set(t.transport.sessions[0]!.id, { shell: "zsh", cwd: null, busy: true, foreground: "npm‮" });
    expect(await t.controller.requestClosePane("p1")).toBe(false);
    expect(t.confirm).toHaveBeenCalledWith('"npm" is still running. Close anyway?');
    expect(t.controller.layout.panes.p1).toBeDefined();
    expect(t.closeWindow).not.toHaveBeenCalled();
  });

  it("busy: null or false does not ask; the last tab closes the window", async () => {
    const t = await setup(false);
    t.transport.infos.set(t.transport.sessions[0]!.id, { shell: "cmd", cwd: null, busy: null, foreground: null });
    t.controller.run({ type: "closePane" });
    await flush();
    expect(t.confirm).not.toHaveBeenCalled();
    expect(t.closeWindow).toHaveBeenCalledOnce();
  });

  it("closeTab asks when any pane is busy", async () => {
    const t = await setup(true);
    t.controller.run({ type: "split", direction: "right" });
    await flush();
    t.controller.run({ type: "newTab" });
    await flush();
    t.transport.infos.set(t.transport.sessions[1]!.id, { shell: "zsh", cwd: null, busy: true, foreground: "ssh" });
    expect(await t.controller.requestCloseTab(t.controller.layout.tabs[0]!.id)).toBe(true);
    expect(t.confirm).toHaveBeenCalledOnce();
    expect(t.controller.layout.tabs).toHaveLength(1);
  });
});

describe("LayoutView", () => {
  it("switching tabs hides and shows without recreating the terminal, toggling WebGL", async () => {
    const t = await setup();
    const firstEl = t.panes[0]!.element;
    t.controller.run({ type: "newTab" });
    await flush();
    const panels = t.root.querySelectorAll<HTMLElement>(".tab-panel");
    expect([...panels].map((p) => p.hidden)).toEqual([true, false]);
    expect(t.panes[0]!.visible.at(-1)).toBe(false);
    t.controller.run({ type: "goToTab", index: 1 });
    expect(t.panes[0]!.visible.at(-1)).toBe(true);
    expect(t.panes).toHaveLength(2);
    expect(t.root.querySelector('[data-pane-id="p1"]')).toBe(firstEl);
    // Splitting moves the existing element into the new split.
    t.controller.run({ type: "split", direction: "right" });
    await flush();
    expect(firstEl.parentElement?.classList.contains("split-row")).toBe(true);
    expect(t.panes).toHaveLength(3);
    expect(firstEl.classList.contains("focused")).toBe(false);
    expect(t.panes[2]!.element.classList.contains("focused")).toBe(true);
  });

  it("leaves the office panel untouched and builds the tab bar with roles", async () => {
    const t = await setup();
    expect(document.getElementById("office-panel")?.hidden).toBe(true);
    expect(t.root.querySelector(".tabbar")?.getAttribute("role")).toBe("tablist");
    expect(t.root.querySelectorAll('[role="tab"]')).toHaveLength(1);
    (t.root.querySelector(".tab-new") as HTMLElement).click();
    await flush();
    expect(t.root.querySelectorAll('[role="tab"]')).toHaveLength(2);
    expect(t.root.querySelector(".tabbar")?.getAttribute("aria-orientation")).toBe("vertical");
    expect(t.root.querySelector(".tabbar")?.firstElementChild?.className).toBe("tab-new");
  });

  it("shows the title and, on a second line, the OSC 7 folder; the shortcut collapses the list", async () => {
    const t = await setup();
    t.panes[0]!.title.emit("vim");
    t.panes[0]!.cwd.emit("src");
    expect(t.root.querySelector(".tab-title")?.textContent).toBe("vim");
    expect(t.root.querySelector(".tab-cwd")?.textContent).toBe("src");
    t.controller.run({ type: "toggleTabs" });
    expect(t.view.tabsCollapsed).toBe(true);
    expect((t.root.querySelector(".tabbar") as HTMLElement).hidden).toBe(true);
    expect(t.root.classList.contains("tabs-collapsed")).toBe(true);
    t.controller.run({ type: "toggleTabs" });
    expect((t.root.querySelector(".tabbar") as HTMLElement).hidden).toBe(false);
    expect(document.getElementById("office-panel")?.hidden).toBe(true);
  });

  it("closing back to one pane drops the old flex share, so it fills the panel", async () => {
    const t = await setup();
    t.controller.run({ type: "split", direction: "right" });
    await flush();
    expect(t.panes[0]!.element.style.flex).toMatch(/^0\.5 1 0/);
    await t.controller.requestClosePane(activeTab(t.controller.layout).focused);
    expect(t.panes[0]!.element.style.flex).toBe("");
    expect(t.panes[0]!.element.parentElement?.classList.contains("tab-panel")).toBe(true);
  });

  it("dragging a divider sets the ratio with a pixel minimum", async () => {
    const t = await setup();
    t.controller.run({ type: "split", direction: "right" });
    await flush();
    const box = t.root.querySelector(".split-row") as HTMLElement;
    box.getBoundingClientRect = () => ({ left: 0, top: 0, width: 400, height: 300 }) as DOMRect;
    const divider = box.querySelector(".divider") as HTMLElement;
    divider.dispatchEvent(new PointerEvent("pointerdown", { pointerId: 1 }));
    divider.dispatchEvent(new PointerEvent("pointermove", { clientX: 10 }));
    const root = activeTab(t.controller.layout).root;
    expect(root.kind === "split" && root.ratio).toBe(0.2); // 80 px of 400
    expect(t.panes[0]!.element.style.flex).toMatch(/^0\.2 1 0/);
    divider.dispatchEvent(new PointerEvent("pointerup"));
  });
});
