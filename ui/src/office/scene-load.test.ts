// @vitest-environment happy-dom
// loadOfficeContent with the PixiJS renderer replaced: the scene when the
// canvas can be created, an empty panel when it cannot.
import { beforeEach, describe, expect, it, vi } from "vitest";

const render = vi.hoisted(() => ({ fail: false, destroyed: 0 }));

vi.mock("./render", () => ({
  PixiRenderer: {
    create: async (container: HTMLElement) => {
      if (render.fail) throw new Error("no WebGL");
      container.append(container.ownerDocument.createElement("canvas"));
      return {
        resize: () => {},
        draw: () => {},
        destroy: () => {
          render.destroyed++;
        },
      };
    },
  },
}));

import { loadOfficeContent } from "./scene";

beforeEach(() => {
  render.fail = false;
  render.destroyed = 0;
  vi.spyOn(console, "error").mockImplementation(() => {});
});

describe("loadOfficeContent", () => {
  it("mounts the scene in the panel's host", async () => {
    const host = document.createElement("div");
    const content = await loadOfficeContent(host);
    expect(host.querySelector("canvas")).not.toBeNull();
    expect(host.classList.contains("office-scene")).toBe(true);
    content.event({ v: 1, session: "s1", agent: "main", kind: "start" });
    content.setVisible(false);
    content.destroy();
    expect(render.destroyed).toBe(1);
  });

  it("falls back to an empty panel when the canvas cannot be created", async () => {
    render.fail = true;
    const host = document.createElement("div");
    host.append(document.createElement("span"));
    const content = await loadOfficeContent(host);
    expect(host.children).toHaveLength(0);
    expect(console.error).toHaveBeenCalled();
    content.snapshot({ v: 1, sessions: [] });
    content.event({ v: 1, session: "s1", agent: "main", kind: "start" });
    content.setVisible(true);
    content.destroy();
  });
});
