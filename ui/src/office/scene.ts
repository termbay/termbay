// The office scene (task C2): what the office panel mounts. Load this module
// with import() when the panel opens for the first time (decision E8); it
// pulls PixiJS in with a second dynamic import, so even a static import of
// this file for its types keeps PixiJS out of the main bundle.
//
//   const { mountOfficeScene } = await import("./office/scene");
//   const scene = await mountOfficeScene(panel);
//   await subscribeOffice(tauriOfficeIpc, scene);
//   scene.setVisible(false); // panel hidden: no frames at all

import type { OfficeContent } from "./content";
import { createOfficeScene, type OfficeSceneHandle, type SceneHost } from "./controller";
import type { Size } from "./layout";
import { createPlaceholder } from "./placeholder";

export type { OfficeScene } from "./controller";

/** The parts of `window` the scene uses; tests pass a fake. */
export type HostWindow = Pick<
  Window,
  "requestAnimationFrame" | "cancelAnimationFrame" | "setTimeout" | "clearTimeout" | "addEventListener" | "removeEventListener"
> & {
  readonly document: Pick<Document, "hidden" | "addEventListener" | "removeEventListener">;
  readonly ResizeObserver?: typeof ResizeObserver;
};

/** Whether `size` is a real one: a hidden panel measures 0 x 0. */
export const realSize = (size: Size): boolean => size.width > 0 && size.height > 0;

/**
 * Frames, size and visibility from the browser, for `container`. Sizes of
 * 0 x 0 (the panel hidden) are never reported; the controller keeps the last
 * real one.
 */
export function browserHost(container: HTMLElement, win: HostWindow = window): SceneHost {
  const doc = win.document;
  const measure = (): Size => ({ width: container.clientWidth, height: container.clientHeight });
  return {
    clock: {
      request: (callback) => win.requestAnimationFrame(callback),
      cancel: (handle) => win.cancelAnimationFrame(handle),
      setTimer: (callback, ms) => win.setTimeout(callback, ms),
      clearTimer: (handle) => win.clearTimeout(handle),
    },
    size: measure(),
    observeSize(onSize) {
      const report = () => {
        const size = measure();
        if (realSize(size)) onSize(size);
      };
      const Observer = win.ResizeObserver;
      if (typeof Observer !== "function") {
        win.addEventListener("resize", report);
        return () => win.removeEventListener("resize", report);
      }
      const observer = new Observer(report);
      observer.observe(container);
      return () => observer.disconnect();
    },
    pageHidden: doc.hidden,
    observePage(onHidden) {
      const listener = () => onHidden(doc.hidden);
      doc.addEventListener("visibilitychange", listener);
      return () => doc.removeEventListener("visibilitychange", listener);
    },
  };
}

/**
 * Mounts the office inside `container` (a canvas that fills it) and returns
 * the scene. `snapshot` and `event` can be given to subscribeOffice as is.
 * Rejects when the canvas cannot be created (neither WebGL nor Canvas 2D).
 */
export async function mountOfficeScene(container: HTMLElement): Promise<OfficeSceneHandle> {
  const host = browserHost(container);
  const { PixiRenderer } = await import("./render");
  const renderer = await PixiRenderer.create(container, host.size);
  container.classList.add("office-scene");
  return createOfficeScene(renderer, host);
}

/**
 * The office panel's content (an OfficeContentLoader): the scene, or an
 * empty panel when the canvas cannot be created, so the terminal never
 * breaks because of the office.
 */
export async function loadOfficeContent(host: HTMLElement): Promise<OfficeContent> {
  try {
    return await mountOfficeScene(host);
  } catch (error) {
    console.error("[termbay] could not draw the office", error);
    return createPlaceholder(host);
  }
}
