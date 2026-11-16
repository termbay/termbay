import type { Disposable } from "./types";

/**
 * Minimal event emitter. With `bufferUntilListener`, events emitted while
 * nobody listens are queued and flushed to the first listener.
 */
export class Emitter<T> {
  private readonly listeners = new Set<(value: T) => void>();
  private pending: T[] | null;

  constructor(options: { bufferUntilListener?: boolean } = {}) {
    this.pending = options.bufferUntilListener ? [] : null;
  }

  on(listener: (value: T) => void): Disposable {
    this.listeners.add(listener);
    if (this.pending && this.pending.length > 0) {
      const queued = this.pending;
      this.pending = null;
      for (const value of queued) listener(value);
    } else {
      this.pending = null;
    }
    return { dispose: () => this.listeners.delete(listener) };
  }

  emit(value: T): void {
    if (this.listeners.size === 0 && this.pending) {
      this.pending.push(value);
      return;
    }
    for (const listener of this.listeners) listener(value);
  }

  clear(): void {
    this.listeners.clear();
    this.pending = null;
  }
}
