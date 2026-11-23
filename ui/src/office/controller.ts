// Glue between the office state, the frame loop and whatever draws (task C2).
// The renderer is injected, so this is tested without WebGL: scene.ts gives
// it the PixiJS one, tests give it a recorder.

import type { OfficeEvent, OfficeListener, OfficeSnapshot } from "../transport/office";
import type { OfficeLayout, Size } from "./layout";
import { FrameLoop, type FrameClock } from "./loop";
import { OfficeModel, POP_MS, type CharacterView } from "./model";

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

/**
 * Everything a frame shows, as a string: two frames with the same key look
 * the same, so the second is not drawn. Positions are rounded to pixels, as
 * the renderer draws them.
 */
export function frameKey(frame: SceneFrame): string {
  const { layout } = frame;
  return JSON.stringify([
    layout.size,
    layout.scale,
    layout.desks,
    frame.characters.map((c) => [
      c.key,
      c.label,
      c.color,
      c.main,
      Math.round(c.x),
      Math.round(c.y),
      c.place,
      c.facing,
      c.seated,
      c.stepFrame,
      c.typeFrame,
      c.dots,
      c.bubbleVisible ? [c.bubble?.mark, c.bubble?.text, c.bubbleAge < POP_MS ? Math.floor(c.bubbleAge / 16) : -1] : null,
      c.cue ? [c.cue, Math.round(c.cueProgress * 30)] : null,
    ]),
  ]);
}

export function createOfficeScene(renderer: SceneRenderer, host: SceneHost): OfficeSceneHandle {
  const model = new OfficeModel(host.size);
  renderer.resize(host.size);
  let time = 0;
  let destroyed = false;
  let drawn = "";
  let force = true;

  const loop = new FrameLoop((dt) => {
    time += dt;
    model.step(dt);
    const frame: SceneFrame = { layout: model.layout, characters: model.characters(), time };
    // Draw only when something on screen changed (or the canvas was resized
    // or shown again).
    const key = frameKey(frame);
    if (force || key !== drawn) {
      renderer.draw(frame);
      drawn = key;
      force = false;
    }
    return model.nextChangeIn();
  }, host.clock);

  const stops: (() => void)[] = [];
  if (host.observeSize) {
    stops.push(
      host.observeSize((size) => {
        // A hidden panel measures 0 x 0: keep the last real size.
        if (destroyed || size.width <= 0 || size.height <= 0) return;
        renderer.resize(size);
        model.resize(size);
        force = true;
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
      if (destroyed) return;
      if (visible) force = true;
      loop.setVisible(visible);
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
