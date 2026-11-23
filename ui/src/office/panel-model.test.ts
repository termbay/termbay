import { describe, expect, it } from "vitest";

import {
  DEFAULT_PANEL_WIDTH,
  MAX_PANEL_WIDTH,
  MIN_PANEL_WIDTH,
  MIN_TERMINAL_WIDTH,
  PanelModel,
  clampWidth,
  hasAgents,
  maxWidth,
} from "./panel-model";

describe("clampWidth", () => {
  it("keeps the panel between its limits", () => {
    expect(clampWidth(400, 1600)).toBe(400);
    expect(clampWidth(10, 1600)).toBe(MIN_PANEL_WIDTH);
    expect(clampWidth(5000, 4000)).toBe(MAX_PANEL_WIDTH);
    expect(clampWidth(400.6, 1600)).toBe(401);
  });

  it("gives the widest panel for a window", () => {
    expect(maxWidth(1000)).toBe(1000 - MIN_TERMINAL_WIDTH);
    expect(maxWidth(5000)).toBe(MAX_PANEL_WIDTH);
    expect(maxWidth(100)).toBe(0);
  });

  it("leaves room for the terminals", () => {
    expect(clampWidth(900, 1000)).toBe(1000 - MIN_TERMINAL_WIDTH);
    // Too narrow for both: the terminals win, the panel never goes negative.
    expect(clampWidth(300, MIN_TERMINAL_WIDTH + 100)).toBe(100);
    expect(clampWidth(300, 200)).toBe(0);
  });

  it("uses the default for a width that is not a number", () => {
    expect(clampWidth(Number.NaN, 1600)).toBe(DEFAULT_PANEL_WIDTH);
  });
});

describe("hasAgents", () => {
  it("is true only when some session has an agent", () => {
    expect(hasAgents({ v: 1, sessions: [] })).toBe(false);
    expect(hasAgents({ v: 1, sessions: [{ session: "s", agents: [], tasks: [] }] })).toBe(false);
    expect(hasAgents({ v: 1, sessions: [{ session: "s", agents: [{ id: "main", activity: "idle" }], tasks: [] }] })).toBe(true);
  });
});

describe("PanelModel", () => {
  it("starts closed and opens by itself on the first agent only", () => {
    const model = new PanelModel({ autoOpen: true });
    expect(model.open).toBe(false);
    expect(model.agentSeen()).toBe(true);
    expect(model.open).toBe(true);

    // Closed by the user, it stays closed when more agents come.
    model.close();
    expect(model.agentSeen()).toBe(false);
    expect(model.open).toBe(false);
  });

  it("does not open by itself after the person closed it, even before any agent", () => {
    const model = new PanelModel({ autoOpen: true });
    model.toggle();
    model.toggle();
    expect(model.agentSeen()).toBe(false);
    expect(model.open).toBe(false);

    const other = new PanelModel({ autoOpen: true });
    other.close();
    expect(other.agentSeen()).toBe(false);
    expect(other.open).toBe(false);
  });

  it("does not open by itself with auto_open off", () => {
    const model = new PanelModel({ autoOpen: false });
    expect(model.agentSeen()).toBe(false);
    expect(model.open).toBe(false);
    expect(model.toggle()).toBe(true);
    expect(model.toggle()).toBe(false);
  });

  it("an agent does not report opening a panel that is already open", () => {
    const model = new PanelModel({ autoOpen: true });
    model.toggle();
    expect(model.agentSeen()).toBe(false);
    expect(model.open).toBe(true);
  });

  it("remembers a width and fits it to the window", () => {
    expect(new PanelModel({ autoOpen: true }).width).toBe(DEFAULT_PANEL_WIDTH);
    expect(new PanelModel({ autoOpen: true, width: -3 }).width).toBe(DEFAULT_PANEL_WIDTH);
    expect(new PanelModel({ autoOpen: true, width: Number.NaN }).width).toBe(DEFAULT_PANEL_WIDTH);

    const model = new PanelModel({ autoOpen: true, width: 500 });
    expect(model.width).toBe(500);
    expect(model.widthFor(2000)).toBe(500);
    // A smaller window shrinks what is drawn, not what was chosen.
    expect(model.widthFor(700)).toBe(700 - MIN_TERMINAL_WIDTH);
    expect(model.width).toBe(500);

    expect(model.resize(50, 2000)).toBe(MIN_PANEL_WIDTH);
    expect(model.resize(9999, 2000)).toBe(MAX_PANEL_WIDTH);
    expect(model.width).toBe(MAX_PANEL_WIDTH);
  });
});
