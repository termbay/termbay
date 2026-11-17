import { describe, expect, it } from "vitest";
import { cwdLabelFromOsc7, sanitizeTitle } from "./title";

describe("sanitizeTitle", () => {
  it("removes ESC, C0, C1 and bidi controls, and limits the length", () => {
    expect(sanitizeTitle("\x1b[31mvim\x07 ‮main.rs⁦")).toBe("[31mvim main.rs");
    expect(sanitizeTitle("a\u0085b‏c")).toBe("abc");
    expect(sanitizeTitle("x".repeat(200))).toHaveLength(80);
    expect(sanitizeTitle("🙂".repeat(100))).toBe("🙂".repeat(80));
    expect(sanitizeTitle("\x1b\x07  ")).toBeNull();
  });
});

describe("cwdLabelFromOsc7", () => {
  it("shows the last path component, decoded", () => {
    expect(cwdLabelFromOsc7("file://host/Users/me/my%20repo")).toBe("my repo");
    expect(cwdLabelFromOsc7("file:///")).toBe("/");
    expect(cwdLabelFromOsc7("file://host/tmp/")).toBe("tmp");
    expect(cwdLabelFromOsc7("file://h/%E0%A4%A")).toBe("%E0%A4%A");
    expect(cwdLabelFromOsc7("http://evil/x")).toBeNull();
    expect(cwdLabelFromOsc7("file://h/a/%1b%5b2J‮")).toBe("[2J");
  });
});
