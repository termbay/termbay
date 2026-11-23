// Fails when a build folder has what a release must not ship. Run after every
// release build: `npm run check:release`.
//
// - Anywhere in the build: e2e-only code (the WebGL stub in e2e/webgl-stub.ts,
//   the office debug list in src/office/debug.ts) and the office demo
//   (office-demo.html and src/office/demo-page.ts, dev only).
// - In the initial bundle (what index.html loads: its scripts, modulepreloads
//   and their static imports): PixiJS. The office scene and PixiJS are only
//   reached through import() when the panel opens (decision E8), so their
//   lazy chunks may contain it.
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { basename, dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

/** Must not be in any file of the build. */
export const MARKERS = [
  "termbay-e2e-dom-renderer",
  "termbay-e2e-office-debug",
  "termbay-e2e-office-probe",
  "termbay-office-demo",
];
/** Must not be a file of the build. */
export const FORBIDDEN_FILES = [/^office-demo/];
/** PixiJS's own name, as its code spells it ("pixi.js", "PixiJS"). */
export const PIXI = /pixi\.?js/i;

const walk = (dir, out = []) => {
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) walk(path, out);
    else out.push(path);
  }
  return out;
};

// Static imports only: `import x from "./a.js"`, `import "./a.js"`,
// `export * from "./a.js"`. `import("./a.js")` is lazy and not followed.
const STATIC_IMPORT = /(?:^|[^\w$.])(?:import|export)\s*(?:[\w$*{}\s,]+?\s*from\s*)?["']([^"']+)["']/g;
const HTML_ENTRY = /<(?:script[^>]*\bsrc|link[^>]*\brel=["']?modulepreload["']?[^>]*\bhref)=["']([^"']+)["']/gi;

/** Files index.html loads before anything is opened. */
export function initialFiles(root) {
  const html = join(root, "index.html");
  if (!existsSync(html)) return [];
  const queue = [];
  for (const [, src] of readFileSync(html, "utf8").matchAll(HTML_ENTRY)) {
    if (!/^[a-z]+:/i.test(src)) queue.push(resolve(root, src.replace(/^\//, "")));
  }
  const seen = new Set();
  while (queue.length > 0) {
    const file = queue.shift();
    if (seen.has(file) || !existsSync(file)) continue;
    seen.add(file);
    if (!file.endsWith(".js")) continue;
    for (const [, spec] of readFileSync(file, "utf8").matchAll(STATIC_IMPORT)) {
      if (spec.startsWith(".") || spec.startsWith("/")) queue.push(resolve(dirname(file), spec));
    }
  }
  return [...seen];
}

/** Every problem found in `root`, as readable lines; empty when the build is fine. */
export function checkRelease(root) {
  const hits = [];
  for (const path of walk(root)) {
    const name = relative(root, path);
    if (FORBIDDEN_FILES.some((re) => re.test(basename(path)))) hits.push(`${name} (dev-only file)`);
    const content = readFileSync(path);
    for (const marker of MARKERS) if (content.includes(marker)) hits.push(`${name} (${marker})`);
  }
  for (const path of initialFiles(root)) {
    if (PIXI.test(readFileSync(path, "utf8"))) hits.push(`${relative(root, path)} (PixiJS in the initial bundle)`);
  }
  return hits;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const root = process.argv[2] ?? "dist";
  const hits = checkRelease(root);
  if (hits.length > 0) {
    console.error(`error: the build has what a release must not ship: ${hits.join(", ")}`);
    process.exit(1);
  }
  console.log(`ok: ${root} has no e2e-only or dev-only code, and no PixiJS in the initial bundle`);
}
