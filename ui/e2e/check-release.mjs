// Fails when a build folder contains e2e-only code: the WebGL stub (see
// e2e/webgl-stub.ts) or the office debug list (src/office/debug.ts). Run
// after every release build: `npm run check:release`.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";

const MARKERS = ["termbay-e2e-dom-renderer", "termbay-e2e-office-debug"];
const root = process.argv[2] ?? "dist";
const hits = [];
const walk = (dir) => {
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) walk(path);
    else {
      const content = readFileSync(path);
      for (const marker of MARKERS) if (content.includes(marker)) hits.push(`${path} (${marker})`);
    }
  }
};
walk(root);
if (hits.length > 0) {
  console.error(`error: e2e-only code is in the build: ${hits.join(", ")}`);
  process.exit(1);
}
console.log(`ok: ${root} has no e2e-only code`);
