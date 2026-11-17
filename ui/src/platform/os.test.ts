import { describe, expect, it } from "vitest";
import { detectOs } from "./os";

describe("detectOs", () => {
  it("prefers userAgentData when present", () => {
    expect(detectOs({ userAgentData: { platform: "Windows" } })).toBe("windows");
    expect(detectOs({ userAgentData: { platform: "macOS" } })).toBe("macos");
  });

  it("falls back to navigator.platform and the user agent", () => {
    expect(detectOs({ platform: "MacIntel" })).toBe("macos");
    expect(detectOs({ platform: "Win32" })).toBe("windows");
    expect(detectOs({ platform: "Linux x86_64" })).toBe("linux");
    expect(detectOs({ userAgent: "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15" })).toBe("linux");
  });

  it("treats unknown platforms as linux", () => {
    expect(detectOs({})).toBe("linux");
  });
});
