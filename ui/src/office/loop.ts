// The frame loop of the office (task C2, decision E8): asks for frames only
// while the panel is visible, the page is not hidden (window minimized or in
// the background) and something moves. Otherwise no requestAnimationFrame is
// pending at all, so a hidden or still office costs nothing.

/** Frames per second the office draws at most; pixel art needs no more. */
export const MAX_FPS = 30;
/** Longest step given to one frame, so a stall does not teleport anyone. */
export const MAX_STEP_MS = 100;

export interface FrameClock {
  request(callback: (time: number) => void): number;
  cancel(handle: number): void;
}

/**
 * `frame(dt)` advances and draws; it returns whether more frames are needed
 * (something still moves). `wake()` after any change.
 */
export class FrameLoop {
  private visible = true;
  private pageHidden = false;
  private handle: number | null = null;
  private last: number | null = null;
  private pending = 0;
  private dirty = false;
  private destroyed = false;
  // A little under the interval, so display frames that come a hair early
  // (timer jitter, 59.9 Hz) are not skipped, which would halve the rate.
  private readonly minInterval = 1000 / MAX_FPS - 2;

  constructor(
    private readonly frame: (dt: number) => boolean,
    private readonly clock: FrameClock,
  ) {}

  /** Whether a frame is scheduled. */
  get running(): boolean {
    return this.handle !== null;
  }

  /** Whether frames are allowed at all (visible panel, visible page). */
  get allowed(): boolean {
    return this.visible && !this.pageHidden && !this.destroyed;
  }

  setVisible(visible: boolean): void {
    this.visible = visible;
    this.update();
  }

  setPageHidden(hidden: boolean): void {
    this.pageHidden = hidden;
    this.update();
  }

  /** Something changed: draw it (now, if allowed). */
  wake(): void {
    this.dirty = true;
    this.schedule();
  }

  destroy(): void {
    this.destroyed = true;
    this.stop();
  }

  private update(): void {
    if (this.allowed) this.wake();
    else this.stop();
  }

  private schedule(): void {
    if (this.handle !== null || !this.allowed) return;
    this.handle = this.clock.request((time) => this.run(time));
  }

  private stop(): void {
    if (this.handle !== null) this.clock.cancel(this.handle);
    this.handle = null;
    // The next frame after a pause starts a fresh step, without the pause in it.
    this.last = null;
    this.pending = 0;
  }

  private run(time: number): void {
    this.handle = null;
    if (!this.allowed) return;
    const dt = this.last === null ? 0 : Math.min(MAX_STEP_MS, Math.max(0, time - this.last));
    this.last = time;
    this.pending += dt;
    // Under MAX_FPS: skip this display frame, unless a change is waiting.
    if (!this.dirty && this.pending < this.minInterval) {
      this.schedule();
      return;
    }
    const step = Math.min(MAX_STEP_MS, this.pending);
    this.pending = 0;
    this.dirty = false;
    const more = this.frame(step);
    if (more) this.schedule();
    else this.last = null;
  }
}
