// Office content of release builds until the scene exists (task C2): an
// empty panel. Loaded with import() on the first opening, like the scene.

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
