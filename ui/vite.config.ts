import { defineConfig } from "vitest/config";

// The same static build is embedded by the Tauri window (app/src-tauri)
// and, later, served by `termbay serve`. Relative base keeps both working.
export default defineConfig({
  base: "./",
  build: {
    outDir: "dist",
    target: "es2022",
    sourcemap: false,
    // xterm.js + WebGL addon is ~500 kB minified. It is embedded locally,
    // not downloaded, so a single chunk is fine.
    chunkSizeWarningLimit: 700,
  },
  server: {
    // Dev only. Never exposed beyond loopback.
    host: "127.0.0.1",
    port: 5173,
    strictPort: true,
  },
  test: {
    environment: "node",
    // e2e/*.test.mjs: checks of the build scripts (the e2e specs are in e2e/specs).
    include: ["src/**/*.test.ts", "e2e/*.test.mjs"],
  },
});
