// The frame loop of the office (task C2, decision E8): asks for frames only
// while the panel is visible, the page is not hidden (window minimized or in
// the background) and something is about to change on screen. Otherwise no
// requestAnimationFrame is pending at all, so a hidden or still office costs
// nothing. When the next change is far (a character thinking changes its
// dots every 400 ms), it sleeps on a timer instead of running frames.

/** Frames per second the office draws at most; pixel art needs no more. */
export const MAX_FPS = 30;
/** Longest step given to one frame beyond a planned sleep, so a stall does not teleport anyone. */
export const MAX_STEP_MS = 100;

export interface FrameClock {
  request(callback: (time: number) => void): number;
  cancel(handle: number): void;
  /** A one-shot timer (setTimeout). */
  setTimer(callback: () => void, ms: number): number;
  clearTimer(handle: number): void;
}

/**
 * `frame(dt)` advances and draws; it returns in how many ms the picture
 * changes next: 0 (or anything under a frame) for the next display frame,
 * `Infinity` when nothing will change until the next `wake()`.
 */
export class FrameLoop {
  private visible = true;
  private pageHidden = false;
  private handle: number | null = null;
  private timer: number | null = null;
  private last: number | null = null;
  private pending = 0;
  private dirty = false;
  private slept = 0;
  private destroyed = false;
  // A little under the interval, so display frames that come a hair early
  // (timer jitter, 59.9 Hz) are not skipped, which would halve the rate.
  private readonly minInterval = 1000 / MAX_FPS - 2;

  constructor(
    private readonly frame: (dt: number) => number,
    private readonly clock: FrameClock,
  ) {}

  /** Whether a frame is scheduled now. */
  get running(): boolean {
    return this.handle !== null;
  }

  /** Whether it waits on a timer for a change far ahead. */
  get sleeping(): boolean {
    return this.timer !== null;
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
    this.clearTimer();
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

  private clearTimer(): void {
    if (this.timer !== null) this.clock.clearTimer(this.timer);
    this.timer = null;
  }

  private stop(): void {
    if (this.handle !== null) this.clock.cancel(this.handle);
    this.handle = null;
    this.clearTimer();
    // The next frame after a pause starts a fresh step, without the pause in it.
    this.last = null;
    this.pending = 0;
    this.slept = 0;
  }

  private run(time: number): void {
    this.handle = null;
    if (!this.allowed) return;
    const cap = MAX_STEP_MS + this.slept;
    const dt = this.last === null ? 0 : Math.min(cap, Math.max(0, time - this.last));
    this.last = time;
    this.pending += dt;
    // Under MAX_FPS: skip this display frame, unless a change is waiting.
    if (!this.dirty && this.pending < this.minInterval) {
      this.schedule();
      return;
    }
    const step = Math.min(cap, this.pending);
    this.pending = 0;
    this.dirty = false;
    this.slept = 0;
    const next = this.frame(step);
    if (!(next < Number.POSITIVE_INFINITY)) {
      this.last = null;
    } else if (next <= this.minInterval) {
      this.schedule();
    } else {
      // Far ahead: no frames until then. The step after the sleep may be as
      // long as the sleep itself.
      this.slept = next;
      this.timer = this.clock.setTimer(() => {
        this.timer = null;
        this.schedule();
      }, next);
    }
  }
}
