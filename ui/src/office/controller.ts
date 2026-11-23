// Glue between the office state, the frame loop and whatever draws (task C2).
// The renderer is injected, so this is tested without WebGL: scene.ts gives
// it the PixiJS one, tests give it a recorder.

import type { OfficeEvent, OfficeListener, OfficeSnapshot } from "../transport/office";
import type { OfficeLayout, Size } from "./layout";
import { FrameLoop, type FrameClock } from "./loop";
import { OfficeModel, type CharacterView } from "./model";

/** The office scene, as the panel uses it. `snapshot` and `event` make it an OfficeListener. */
export interface OfficeScene extends OfficeListener {
  snapshot(snapshot: OfficeSnapshot): void;
  event(event: OfficeEvent): void;
  /** `false` stops drawing: no animation frame stays scheduled. */
  setVisible(visible: boolean): void;
  destroy(): void;
}

/** What a renderer gets each frame. */
export interface SceneFrame {
  readonly layout: OfficeLayout;
  /** Top to bottom, the drawing order. */
  readonly characters: readonly CharacterView[];
  /** Scene time in ms (only advances while frames run), for idle animations. */
  readonly time: number;
}

export interface SceneRenderer {
  resize(size: Size): void;
  draw(frame: SceneFrame): void;
  destroy(): void;
}

/** What the page provides: frames, the panel's size and the page's visibility. */
export interface SceneHost {
  readonly clock: FrameClock;
  readonly size: Size;
  /** Calls back with each new size; returns a function that stops it. */
  observeSize?(onSize: (size: Size) => void): () => void;
  /** Whether the page is hidden now (window minimized or in the background). */
  readonly pageHidden?: boolean;
  /** Calls back when the page is hidden or shown; returns a function that stops it. */
  observePage?(onHidden: (hidden: boolean) => void): () => void;
}

export interface OfficeSceneHandle extends OfficeScene {
  /** For tests and the demo page. */
  readonly model: OfficeModel;
  readonly loop: FrameLoop;
}

export function createOfficeScene(renderer: SceneRenderer, host: SceneHost): OfficeSceneHandle {
  const model = new OfficeModel(host.size);
  renderer.resize(host.size);
  let time = 0;
  let destroyed = false;

  const loop = new FrameLoop((dt) => {
    time += dt;
    model.step(dt);
    renderer.draw({ layout: model.layout, characters: model.characters(), time });
    return model.busy();
  }, host.clock);

  const stops: (() => void)[] = [];
  if (host.observeSize) {
    stops.push(
      host.observeSize((size) => {
        // A hidden panel measures 0 x 0: keep the last real size.
        if (destroyed || size.width <= 0 || size.height <= 0) return;
        renderer.resize(size);
        model.resize(size);
        loop.wake();
      }),
    );
  }
  if (host.pageHidden) loop.setPageHidden(true);
  if (host.observePage) stops.push(host.observePage((hidden) => loop.setPageHidden(hidden)));
  loop.wake();

  return {
    model,
    loop,
    snapshot(snapshot) {
      if (destroyed) return;
      model.snapshot(snapshot);
      loop.wake();
    },
    event(event) {
      if (destroyed) return;
      model.event(event);
      loop.wake();
    },
    setVisible(visible) {
      if (!destroyed) loop.setVisible(visible);
    },
    destroy() {
      if (destroyed) return;
      destroyed = true;
      loop.destroy();
      for (const stop of stops) stop();
      renderer.destroy();
    },
  };
}
