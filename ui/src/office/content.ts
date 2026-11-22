// What the office panel shows (task B2 in docs/FASE2-PLANO.md). The panel
// only knows this interface: the PixiJS scene (task C2), the debug list of
// the e2e build and the empty placeholder all implement it, and are loaded
// with import() on the first opening, so a window without agents never
// loads them.

import type { OfficeListener } from "../transport/office";

export interface OfficeContent extends OfficeListener {
  /**
   * Called with false when the panel closes and true when it opens again.
   * Hidden, the content keeps its state but must not draw: no
   * requestAnimationFrame, no timers that render.
   */
  setVisible(visible: boolean): void;
  /** Frees everything; the content is not used again. */
  destroy(): void;
}

/** Builds the content inside `host`, an empty element owned by the panel. */
export type OfficeContentLoader = (host: HTMLElement) => Promise<OfficeContent>;
