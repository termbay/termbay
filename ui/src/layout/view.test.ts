import { describe, expect, it } from "vitest";
import { ratioFromPointer } from "./view";

describe("ratioFromPointer", () => {
  it("keeps both sides at least the minimum in pixels and within 0.1..0.9", () => {
    expect(ratioFromPointer(200, 0, 400, 80)).toBe(0.5);
    expect(ratioFromPointer(10, 0, 400, 80)).toBe(0.2);
    expect(ratioFromPointer(395, 0, 400, 80)).toBe(0.8);
    expect(ratioFromPointer(1, 0, 2000, 80)).toBe(0.1);
    expect(ratioFromPointer(100, 0, 100, 80)).toBe(0.5); // too small for two panes
    expect(ratioFromPointer(5, 0, 0, 80)).toBe(0.5);
  });
});
