import { describe, expect, it } from "vitest";

import { HAIR_STYLES, NAMED_COLORS, SKIN_TONES, hash, lookFor } from "./look";

describe("lookFor", () => {
  it("always gives the same agent the same look", () => {
    expect(lookFor("s1\u0000a1")).toEqual(lookFor("s1\u0000a1"));
    expect(hash("abc")).toBe(hash("abc"));
  });

  it("spreads a team over skin tones and hair styles", () => {
    const looks = Array.from({ length: 40 }, (_, i) => lookFor(`s1\u0000agent-${i}`));
    expect(new Set(looks.map((l) => l.skin)).size).toBeGreaterThanOrEqual(SKIN_TONES.length - 1);
    expect(new Set(looks.map((l) => l.hairStyle)).size).toBeGreaterThanOrEqual(HAIR_STYLES.length - 1);
  });

  it("uses a team color for the shirt, ignoring unknown names", () => {
    expect(lookFor("k", "Green").shirt).toBe(NAMED_COLORS.green);
    expect(lookFor("k", "chartreuse").shirt).toBe(lookFor("k").shirt);
  });
});
