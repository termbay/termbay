// The office scene (task C2): what the office panel mounts. Load this module
// with import() when the panel opens for the first time (decision E8); it
// pulls PixiJS in with a second dynamic import, so even a static import of
// this file for its types keeps PixiJS out of the main bundle.
//
//   const { mountOfficeScene } = await import("./office/scene");
//   const scene = await mountOfficeScene(panel);
//   await subscribeOffice(tauriOfficeIpc, scene);
//   scene.setVisible(false); // panel hidden: no frames at all

import { createOfficeScene, type OfficeScene, type SceneHost } from "./controller";
import type { Size } from "./layout";

export type { OfficeScene } from "./controller";

/** Frames, size and visibility from the browser, for `container`. */
export function browserHost(container: HTMLElement, win: Window = window): SceneHost {
  const doc = win.document;
  const measure = (): Size => ({ width: container.clientWidth, height: container.clientHeight });
  return {
    clock: {
      request: (callback) => win.requestAnimationFrame(callback),
      cancel: (handle) => win.cancelAnimationFrame(handle),
    },
    size: measure(),
    observeSize(onSize) {
      const Observer = (win as Window & { ResizeObserver?: typeof ResizeObserver }).ResizeObserver;
      if (typeof Observer !== "function") {
        const listener = () => onSize(measure());
        win.addEventListener("resize", listener);
        return () => win.removeEventListener("resize", listener);
      }
      const observer = new Observer(() => onSize(measure()));
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
 * Rejects when the canvas cannot be created (no WebGL).
 */
export async function mountOfficeScene(container: HTMLElement): Promise<OfficeScene> {
  const host = browserHost(container);
  const { PixiRenderer } = await import("./render");
  const renderer = await PixiRenderer.create(container, host.size);
  return createOfficeScene(renderer, host);
}
