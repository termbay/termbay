import { fileURLToPath } from "node:url";
import { mergeConfig } from "vite";

import base from "./vite.config";

// Build used only by the end-to-end tests (ui/e2e): same app, but the WebGL
// addon is swapped for a stub so xterm.js renders text in the DOM, where
// WebDriver can read it, and the office debug list is on (task B1). Never
// used for release packages.
export default mergeConfig(base, {
  define: {
    "import.meta.env.VITE_OFFICE_DEBUG": JSON.stringify("1"),
  },
  resolve: {
    alias: {
      "@xterm/addon-webgl": fileURLToPath(new URL("./e2e/webgl-stub.ts", import.meta.url)),
    },
  },
});
