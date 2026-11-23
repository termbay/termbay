// Run by Vitest (npm test), not by the e2e runner: checks e2e/check-release.mjs
// against small fake builds.
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { checkRelease, initialFiles } from "./check-release.mjs";

const roots = [];

/** A fake dist folder with `files` (path to content). */
function build(files) {
  const root = mkdtempSync(join(tmpdir(), "termbay-release-"));
  roots.push(root);
  for (const [path, content] of Object.entries(files)) {
    mkdirSync(dirname(join(root, path)), { recursive: true });
    writeFileSync(join(root, path), content);
  }
  return root;
}

afterEach(() => {
  for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});

const INDEX = `<!doctype html><html><head>
<script type="module" crossorigin src="./assets/index-a.js"></script>
<link rel="modulepreload" crossorigin href="./assets/shared-b.js">
</head><body></body></html>`;

// Like Vite's output: a static import, and the scene behind import().
const ENTRY = `import{s as x}from"./shared-b.js";import"./side-c.js";const o=()=>import("./scene-d.js");x(o);`;
const SCENE = `const r=()=>import("./render-e.js");export{r as mountOfficeScene};`;
const RENDER = `/* pixi.js */ class PixiJS {} export{PixiJS};`;

describe("checkRelease", () => {
  it("passes a normal build, with PixiJS only in the lazy scene chunks", () => {
    const root = build({
      "index.html": INDEX,
      "assets/index-a.js": ENTRY,
      "assets/shared-b.js": "export const s=()=>1;",
      "assets/side-c.js": "console.log(1);",
      "assets/scene-d.js": SCENE,
      "assets/render-e.js": RENDER,
    });
    expect(checkRelease(root)).toEqual([]);
    expect(
      initialFiles(root)
        .map((p) => p.slice(root.length + 1))
        .sort(),
    ).toEqual(["assets/index-a.js", "assets/shared-b.js", "assets/side-c.js"]);
  });

  it("fails when PixiJS is in the entry, a static import or a modulepreload", () => {
    const base = {
      "index.html": INDEX,
      "assets/index-a.js": ENTRY,
      "assets/shared-b.js": "export const s=()=>1;",
      "assets/side-c.js": "console.log(1);",
      "assets/scene-d.js": SCENE,
      "assets/render-e.js": RENDER,
    };
    expect(checkRelease(build({ ...base, "assets/index-a.js": `${ENTRY}/*PixiJS*/` }))).toEqual([
      "assets/index-a.js (PixiJS in the initial bundle)",
    ]);
    expect(checkRelease(build({ ...base, "assets/side-c.js": RENDER }))).toEqual(["assets/side-c.js (PixiJS in the initial bundle)"]);
    expect(checkRelease(build({ ...base, "assets/shared-b.js": `export const s=1;${RENDER}` }))).toEqual([
      "assets/shared-b.js (PixiJS in the initial bundle)",
    ]);
    // The scene imported statically drags PixiJS into the initial bundle.
    const eager = `import{r}from"./render-e.js";${ENTRY}`;
    expect(checkRelease(build({ ...base, "assets/index-a.js": eager }))).toEqual(["assets/render-e.js (PixiJS in the initial bundle)"]);
  });

  it("fails when the office demo is anywhere in the build, lazy or not", () => {
    const root = build({
      "index.html": INDEX,
      "assets/index-a.js": "console.log(1);",
      "assets/late-x.js": `document.body.dataset.marker="termbay-office-demo";`,
      "office-demo.html": "<!doctype html>",
    });
    expect(checkRelease(root).sort()).toEqual(["assets/late-x.js (termbay-office-demo)", "office-demo.html (dev-only file)"]);
  });

  it("still fails on e2e-only code", () => {
    const root = build({ "index.html": INDEX, "assets/index-a.js": `const m="termbay-e2e-office-debug";` });
    expect(checkRelease(root)).toEqual(["assets/index-a.js (termbay-e2e-office-debug)"]);
  });
});
