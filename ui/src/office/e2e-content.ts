// Office content of the e2e build only (VITE_OFFICE_DEBUG=1): the real scene
// on top and the debug list (task B1) below, both fed the same picture and
// changes, plus a probe on window for the e2e specs to compare the scene with
// the core's picture. Release builds load scene.ts instead; the marker below
// lets e2e/check-release.mjs make sure this file never ships.

import { subscribeOffice, tauriOfficeIpc, type OfficeIpc, type OfficeSnapshot } from "../transport/office";
import type { OfficeContent } from "./content";
import type { OfficeSceneHandle } from "./controller";
import { OfficeDebugList } from "./debug";
import { summarize, type SessionSummary } from "./model";
import { mountOfficeScene } from "./scene";

/** Found by e2e/check-release.mjs: must never be in a release build. */
export const OFFICE_PROBE_MARKER = "termbay-e2e-office-probe";

/** What the e2e specs read through `window.__termbayOffice`. */
export interface OfficeProbe {
  readonly marker: string;
  /** Whether the scene has its canvas. */
  canvas(): boolean;
  /** The scene's agents, or null when the scene could not be drawn. */
  scene(): SessionSummary[] | null;
  /** The core's picture now (a fresh subscription, cancelled at once), in the same shape. */
  core(): Promise<SessionSummary[]>;
}

/** The core's current picture, through a subscription dropped right after. */
export async function corePicture(ipc: OfficeIpc): Promise<OfficeSnapshot> {
  const controller = new AbortController();
  let picture: OfficeSnapshot | null = null;
  await subscribeOffice(
    ipc,
    {
      snapshot: (s) => {
        picture = s;
        controller.abort();
      },
      event: () => {},
    },
    controller.signal,
  );
  return picture ?? { v: 1, sessions: [] };
}

export async function createE2EContent(
  host: HTMLElement,
  options: { ipc?: OfficeIpc; mount?: (container: HTMLElement) => Promise<OfficeSceneHandle>; target?: object } = {},
): Promise<OfficeContent> {
  const { ipc = tauriOfficeIpc, mount = mountOfficeScene, target = window } = options;
  const doc = host.ownerDocument;
  const sceneHost = doc.createElement("div");
  sceneHost.style.cssText = "position:absolute;left:0;right:0;top:0;height:60%;";
  const debugHost = doc.createElement("div");
  debugHost.style.cssText = "position:absolute;left:0;right:0;bottom:0;height:40%;";
  debugHost.dataset.marker = OFFICE_PROBE_MARKER;
  host.replaceChildren(sceneHost, debugHost);

  const list = new OfficeDebugList(debugHost);
  let scene: OfficeSceneHandle | null = null;
  try {
    scene = await mount(sceneHost);
  } catch (error) {
    console.error("[termbay] e2e: could not draw the office", error);
  }

  const probe: OfficeProbe = {
    marker: OFFICE_PROBE_MARKER,
    canvas: () => sceneHost.querySelector("canvas") !== null,
    scene: () => scene?.model.agents() ?? null,
    core: async () => summarize(await corePicture(ipc)),
  };
  (target as { __termbayOffice?: OfficeProbe }).__termbayOffice = probe;

  return {
    snapshot(snapshot) {
      list.snapshot(snapshot);
      scene?.snapshot(snapshot);
    },
    event(event) {
      list.event(event);
      scene?.event(event);
    },
    setVisible(visible) {
      scene?.setVisible(visible);
    },
    destroy() {
      scene?.destroy();
      host.replaceChildren();
      delete (target as { __termbayOffice?: OfficeProbe }).__termbayOffice;
    },
  };
}
