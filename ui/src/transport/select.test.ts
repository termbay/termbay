import { describe, expect, it } from "vitest";
import { EchoPtyTransport } from "./echo";
import { createTransport } from "./select";
import { TauriPtyTransport } from "./tauri";

describe("createTransport", () => {
  it("uses Tauri inside the Tauri window", () => {
    expect(createTransport({ isTauri: () => true })).toBeInstanceOf(TauriPtyTransport);
  });

  it("uses the echo transport in a plain browser", () => {
    expect(createTransport({ isTauri: () => false })).toBeInstanceOf(EchoPtyTransport);
  });

  it("detects a plain Node/browser environment as not Tauri by default", () => {
    expect(createTransport()).toBeInstanceOf(EchoPtyTransport);
  });
});
