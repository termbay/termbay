// Replaces @xterm/addon-webgl in the e2e build only (vite.e2e.config.ts).
// WebGL draws the terminal text on a canvas, which WebDriver cannot read, so
// the constructor fails and RendererController falls back to xterm.js's DOM
// renderer (`.xterm-rows`). The marker below must never reach a release
// build: the CI greps ui/dist for it after every normal build.
export const E2E_MARKER = "termbay-e2e-dom-renderer";

export class WebglAddon {
  constructor() {
    throw new Error(`${E2E_MARKER}: WebGL disabled in the e2e build`);
  }
  onContextLoss(): { dispose(): void } {
    return { dispose() {} };
  }
  activate(): void {}
  dispose(): void {}
}
