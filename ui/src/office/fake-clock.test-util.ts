// A frame clock driven by hand, for the loop and scene tests.

import type { FrameClock } from "./loop";

export class FakeClock implements FrameClock {
  private readonly callbacks = new Map<number, (time: number) => void>();
  private nextHandle = 1;
  now = 0;
  /** requestAnimationFrame calls so far. */
  requests = 0;

  request(callback: (time: number) => void): number {
    const handle = this.nextHandle++;
    this.callbacks.set(handle, callback);
    this.requests++;
    return handle;
  }

  cancel(handle: number): void {
    this.callbacks.delete(handle);
  }

  /** Frames waiting to run. */
  get pending(): number {
    return this.callbacks.size;
  }

  /** One display frame, `ms` after the last. */
  tick(ms = 1000 / 60): void {
    this.now += ms;
    const callbacks = [...this.callbacks.values()];
    this.callbacks.clear();
    for (const callback of callbacks) callback(this.now);
  }

  /** `n` display frames. */
  run(n: number, ms = 1000 / 60): void {
    for (let i = 0; i < n; i++) this.tick(ms);
  }
}
