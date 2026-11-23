// @vitest-environment happy-dom
import { describe, expect, it, vi } from "vitest";

import type { OfficeEvent, OfficeIpc, OfficeSnapshot } from "../transport/office";
import { createOfficeScene, type OfficeSceneHandle, type SceneRenderer } from "./controller";
import { OFFICE_PROBE_MARKER, corePicture, createE2EContent, type OfficeProbe } from "./e2e-content";
import { FakeClock } from "./fake-clock.test-util";

const renderer: SceneRenderer = { resize: () => {}, draw: () => {}, destroy: () => {} };

/** A real scene with a do-nothing renderer, and a canvas like PixiJS's. */
const mount = async (container: HTMLElement): Promise<OfficeSceneHandle> => {
  container.append(container.ownerDocument.createElement("canvas"));
  return createOfficeScene(renderer, { clock: new FakeClock(), size: { width: 320, height: 400 } });
};

const picture: OfficeSnapshot = {
  v: 1,
  sessions: [{ session: "s1", agents: [{ id: "main", activity: "working", bubble: "reading api.ts" }], tasks: [] }],
};

function ipcWith(snapshot: OfficeSnapshot): OfficeIpc & { sent: (e: OfficeEvent) => void } {
  let send: (e: OfficeEvent) => void = () => {};
  return {
    channel: (onMessage) => {
      send = onMessage;
      return {};
    },
    invoke: async <T>() => snapshot as T,
    sent: (e) => send(e),
  };
}

const ev = (fields: Partial<OfficeEvent>): OfficeEvent => ({ v: 1, session: "s1", agent: "main", kind: "tool", ...fields });

describe("createE2EContent", () => {
  it("feeds the scene and the debug list, and exposes a probe", async () => {
    const host = document.createElement("div");
    const target: { __termbayOffice?: OfficeProbe } = {};
    const content = await createE2EContent(host, { ipc: ipcWith(picture), mount, target });
    content.snapshot(picture);
    content.event(ev({ action: "reading", target: "x.ts" }));

    expect([...host.querySelectorAll(".office-debug li")].map((li) => li.textContent)).toEqual([
      "here \u00b7 main \u00b7 working \u00b7 reading api.ts",
      "tool \u00b7 main \u00b7 reading \u00b7 x.ts",
    ]);
    const probe = target.__termbayOffice as OfficeProbe;
    expect(probe.marker).toBe(OFFICE_PROBE_MARKER);
    expect(probe.canvas()).toBe(true);
    expect(probe.scene()).toEqual([{ session: "s1", agents: [{ id: "main", activity: "working", bubble: "reading x.ts" }] }]);
    expect(await probe.core()).toEqual([{ session: "s1", agents: [{ id: "main", activity: "working", bubble: "reading api.ts" }] }]);

    content.destroy();
    expect(host.children).toHaveLength(0);
    expect(target.__termbayOffice).toBeUndefined();
  });

  it("keeps the debug list when the scene cannot be drawn", async () => {
    // The failure is logged on purpose; keep the test output clean.
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    const host = document.createElement("div");
    const target: { __termbayOffice?: OfficeProbe } = {};
    const content = await createE2EContent(host, {
      ipc: ipcWith(picture),
      mount: () => Promise.reject(new Error("no WebGL")),
      target,
    });
    content.event(ev({ action: "reading", target: "y.ts" }));
    expect(host.querySelector(".office-debug li")?.textContent).toBe("tool \u00b7 main \u00b7 reading \u00b7 y.ts");
    expect(target.__termbayOffice?.canvas()).toBe(false);
    expect(target.__termbayOffice?.scene()).toBeNull();
    content.setVisible(false);
    content.destroy();
    expect(error).toHaveBeenCalled();
    error.mockRestore();
  });
});

describe("corePicture", () => {
  it("returns the picture and ignores later changes", async () => {
    const ipc = ipcWith(picture);
    expect(await corePicture(ipc)).toEqual(picture);
    expect(() => ipc.sent(ev({}))).not.toThrow();
  });
});
