import { describe, expect, it } from "vitest";

import { CELL, DESK_FRONT, MIN_SCALE, breakSpot, columnsFor, inCorridor, layoutOffice, route, type Point, type Rect } from "./layout";

const PANEL = { width: 320, height: 600 };
const inside = (p: Point, r: Rect) => p.x >= r.x && p.x <= r.x + r.width && p.y >= r.y && p.y <= r.y + r.height;

describe("layoutOffice", () => {
  it("has a corridor at the bottom with the door on the left and coffee on the right", () => {
    const layout = layoutOffice(PANEL, []);
    expect(layout.corridor.y + layout.corridor.height).toBe(PANEL.height);
    expect(layout.door.x).toBe(0);
    expect(inCorridor(layout, layout.door)).toBe(true);
    expect(layout.coffee.x + layout.coffee.width).toBe(PANEL.width);
    expect(layout.floor.y + layout.floor.height).toBeLessThan(layout.corridor.y);
    expect(layout.desks).toEqual([]);
  });

  it("gives one desk per agent, inside its session's zone, none overlapping", () => {
    for (const n of [1, 2, 3, 5, 8, 13, 30]) {
      const layout = layoutOffice(PANEL, [n]);
      const desks = layout.desks[0] ?? [];
      expect(desks).toHaveLength(n);
      const zone = layout.zones[0] as Rect;
      for (const d of desks) expect(inside(d, zone), `${n} desks`).toBe(true);
      const minGap = Math.min(CELL.width, CELL.height) * layout.scale * 0.9;
      for (let i = 0; i < n; i++)
        for (let j = i + 1; j < n; j++) {
          const a = desks[i] as Point;
          const b = desks[j] as Point;
          expect(Math.max(Math.abs(a.x - b.x), Math.abs(a.y - b.y)), `${n} desks`).toBeGreaterThanOrEqual(minGap);
        }
    }
  });

  it("puts a lone agent in the middle of the floor at full scale", () => {
    const layout = layoutOffice(PANEL, [1]);
    const desk = layout.desks[0]?.[0] as Point;
    expect(desk.x).toBeCloseTo(PANEL.width / 2);
    expect(layout.scale).toBe(1);
  });

  it("shrinks with many agents, but never below MIN_SCALE", () => {
    expect(layoutOffice(PANEL, [4]).scale).toBe(1);
    expect(layoutOffice(PANEL, [30]).scale).toBeLessThan(1);
    expect(layoutOffice({ width: 120, height: 160 }, [64]).scale).toBe(MIN_SCALE);
  });

  it("splits sessions along the longer side", () => {
    const tall = layoutOffice(PANEL, [2, 3]);
    expect(tall.zones).toHaveLength(2);
    expect(tall.zones[1]?.y).toBeGreaterThan(tall.zones[0]?.y ?? 0);
    expect(tall.desks.map((d) => d.length)).toEqual([2, 3]);
    const wide = layoutOffice({ width: 900, height: 300 }, [2, 3]);
    expect(wide.zones[1]?.x).toBeGreaterThan(wide.zones[0]?.x ?? 0);
  });

  it("seats each character behind its desk, so the desk is drawn over it", () => {
    const layout = layoutOffice(PANEL, [3]);
    layout.desks[0]?.forEach((d, i) => {
      const seat = layout.seats[0]?.[i] as Point;
      expect(seat.x).toBe(d.x);
      expect(seat.y).toBeGreaterThan(d.y);
      expect(seat.y).toBeLessThan(d.y + DESK_FRONT * layout.scale);
      expect(inCorridor(layout, seat)).toBe(false);
    });
  });

  it("survives a hidden (0 x 0) panel", () => {
    const layout = layoutOffice({ width: 0, height: 0 }, [3]);
    expect(layout.desks[0]).toHaveLength(3);
    for (const d of layout.desks[0] ?? []) expect(Number.isFinite(d.x) && Number.isFinite(d.y)).toBe(true);
  });
});

describe("columnsFor", () => {
  it("prefers columns in wide areas and rows in tall ones", () => {
    expect(columnsFor(1, 300, 300)).toBe(1);
    expect(columnsFor(4, 1000, 80)).toBe(4);
    expect(columnsFor(4, 90, 1000)).toBe(1);
  });
});

describe("breakSpot", () => {
  it("lines free agents up in the corridor, by the coffee, without stacking", () => {
    const layout = layoutOffice(PANEL, [6]);
    const spots = Array.from({ length: 8 }, (_, i) => breakSpot(layout, i));
    for (const s of spots) expect(inCorridor(layout, s)).toBe(true);
    expect(spots[0]?.x).toBeGreaterThan(PANEL.width / 2);
    const keys = new Set(spots.map((s) => `${Math.round(s.x)},${Math.round(s.y)}`));
    expect(keys.size).toBe(spots.length);
  });
});

describe("route", () => {
  const layout = layoutOffice(PANEL, [2]);
  const lane = layout.corridor.y + layout.corridor.height / 2;

  it("walks from the door to a desk through the corridor", () => {
    const seat = layout.seats[0]?.[1] as Point;
    const path = route(layout, layout.door, seat);
    expect(path.at(-1)).toEqual(seat);
    expect(path.slice(0, -1).every((p) => Math.abs(p.y - lane) < 0.5)).toBe(true);
  });

  it("goes straight inside the corridor, or when asked", () => {
    const a = breakSpot(layout, 0);
    expect(route(layout, layout.door, a)).toEqual([a]);
    const seat = layout.seats[0]?.[0] as Point;
    const near = { x: seat.x + 10, y: seat.y };
    expect(route(layout, near, seat, true)).toEqual([seat]);
    expect(route(layout, seat, seat)).toEqual([]);
  });
});
