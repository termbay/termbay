// An empty office panel: what release builds show when the scene cannot
// draw (no WebGL nor Canvas 2D), so the office never breaks the terminal.

import type { OfficeContent } from "./content";

export function createPlaceholder(host: HTMLElement): OfficeContent {
  host.replaceChildren();
  return {
    snapshot: () => {},
    event: () => {},
    setVisible: () => {},
    destroy: () => host.replaceChildren(),
  };
}
