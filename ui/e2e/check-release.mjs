// Fails when a build folder contains the e2e-only WebGL stub (see
// e2e/webgl-stub.ts). Run after every release build: `npm run check:release`.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";

const MARKER = "termbay-e2e-dom-renderer";
const root = process.argv[2] ?? "dist";
const hits = [];
const walk = (dir) => {
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) walk(path);
    else if (readFileSync(path).includes(MARKER)) hits.push(path);
  }
};
walk(root);
if (hits.length > 0) {
  console.error(`error: the e2e WebGL stub is in the build: ${hits.join(", ")}`);
  process.exit(1);
}
console.log(`ok: ${root} has no e2e-only code`);
