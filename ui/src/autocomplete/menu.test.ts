import { describe, expect, it } from "vitest";
import { anchorCell, computeMenuPosition, groupEntries, type PositionInput } from "./menu";

/** 80x24 terminal, 8x16 px cells, screen at (6, 4) inside a 646x388 pane. */
const base: PositionInput = {
  col: 10,
  row: 2,
  rows: 24,
  cellWidth: 8,
  cellHeight: 16,
  screenLeft: 6,
  screenTop: 4,
  areaWidth: 646,
  areaHeight: 388,
  menuWidth: 200,
  menuHeight: 100,
};

describe("computeMenuPosition", () => {
  it("opens below the cursor cell", () => {
    expect(computeMenuPosition(base)).toEqual({
      left: 6 + 10 * 8,
      top: 4 + 3 * 16,
      placement: "below",
      maxHeight: 388 - (4 + 3 * 16),
      maxWidth: 646,
    });
  });

  it("opens above near the last row", () => {
    const p = computeMenuPosition({ ...base, row: 23 });
    const rowTop = 4 + 23 * 16;
    expect(p?.placement).toBe("above");
    expect(p?.top).toBe(rowTop - 100);
    expect(p?.maxHeight).toBe(rowTop);
  });

  it("stays below, limited in height, when there is more room below than above", () => {
    const p = computeMenuPosition({ ...base, row: 3, menuHeight: 1000 });
    expect(p?.placement).toBe("below");
    expect(p?.maxHeight).toBe(388 - (4 + 4 * 16));
  });

  it("goes above, limited to the room above, for a tall menu on a low row", () => {
    const p = computeMenuPosition({ ...base, row: 20, menuHeight: 1000 });
    expect(p?.placement).toBe("above");
    expect(p?.top).toBe(0);
    expect(p?.maxHeight).toBe(4 + 20 * 16);
  });

  it("shifts left to stay inside the pane on the right", () => {
    const p = computeMenuPosition({ ...base, col: 79 });
    expect(p?.left).toBe(646 - 200);
  });

  it("never starts left of the pane, even for a menu wider than it", () => {
    const p = computeMenuPosition({ ...base, col: 70, menuWidth: 900 });
    expect(p?.left).toBe(0);
    expect(p?.maxWidth).toBe(646);
  });

  it("is null when the cursor row is scrolled out of view or the cell size is unknown", () => {
    expect(computeMenuPosition({ ...base, row: -1 })).toBeNull();
    expect(computeMenuPosition({ ...base, row: 24 })).toBeNull();
    expect(computeMenuPosition({ ...base, cellWidth: 0 })).toBeNull();
  });
});

describe("anchorCell", () => {
  const cursor = { cursorX: 12, cursorY: 5, baseY: 100, viewportY: 100, cols: 80, rows: 24 };

  it("anchors at the start of the word on the cursor row", () => {
    expect(anchorCell(cursor, 2)).toEqual({ col: 10, row: 5 });
  });

  it("anchors at the cursor when the word wraps from the previous row", () => {
    expect(anchorCell({ ...cursor, cursorX: 1 }, 4)).toEqual({ col: 1, row: 5 });
  });

  it("gives the row relative to the viewport when scrolled up", () => {
    expect(anchorCell({ ...cursor, viewportY: 97 }, 0).row).toBe(8);
    expect(anchorCell({ ...cursor, viewportY: 50 }, 0).row).toBe(55);
  });

  it("keeps the column inside the terminal (cursor parked past the last column)", () => {
    expect(anchorCell({ ...cursor, cursorX: 80 }, 0).col).toBe(79);
  });
});

describe("groupEntries", () => {
  it("splits shell completions and history, keeping flat indices", () => {
    const groups = groupEntries([
      { text: "checkout", kind: "shell" },
      { text: "cherry", kind: "shell" },
      { text: "git checkout main", kind: "history" },
    ]);
    expect(groups.map((g) => [g.kind, g.entries.map((e) => e.index)])).toEqual([
      ["shell", [0, 1]],
      ["history", [2]],
    ]);
  });

  it("omits empty sections", () => {
    expect(groupEntries([{ text: "git status", kind: "history" }]).map((g) => g.kind)).toEqual(["history"]);
  });
});
