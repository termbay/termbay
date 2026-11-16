// Tests for check-npm-licenses.mjs. Run with: node --test "scripts/*.test.mjs"
import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync, mkdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { parseSpdx, satisfies, collectPackages, checkPackages } from "./check-npm-licenses.mjs";

const SCRIPT = join(dirname(fileURLToPath(import.meta.url)), "check-npm-licenses.mjs");

const policy = {
  allow: ["MIT", "Apache-2.0", "ISC", "Apache-2.0 WITH LLVM-exception"],
  exceptions: [{ package: "css-tool-*", license: "MPL-2.0", devOnly: true, reason: "test" }],
};

const allowed = (expr) => satisfies(parseSpdx(expr), (id) => policy.allow.includes(id));

function project(packages) {
  const dir = mkdtempSync(join(tmpdir(), "termbay-licenses-"));
  const lock = { name: "fixture", lockfileVersion: 3, packages: { "": { name: "fixture" }, ...packages } };
  writeFileSync(join(dir, "package-lock.json"), JSON.stringify(lock));
  return dir;
}

test("SPDX: OR passes if one side is allowed, AND needs both", () => {
  assert.equal(allowed("MIT"), true);
  assert.equal(allowed("(MIT OR GPL-3.0-only)"), true);
  assert.equal(allowed("MIT AND GPL-3.0-only"), false);
  assert.equal(allowed("GPL-2.0-or-later"), false);
  assert.equal(allowed("(MIT OR GPL-3.0-only) AND (ISC OR AGPL-3.0-only)"), true);
  assert.equal(allowed("Apache-2.0 WITH LLVM-exception"), true);
  assert.equal(allowed("GPL-2.0-only WITH Classpath-exception-2.0"), false);
  assert.equal(allowed("MIT OR Apache-2.0 AND GPL-3.0-only"), true); // AND binds tighter
});

test("SPDX: invalid expressions throw", () => {
  for (const bad of ["", "MIT OR", "(MIT", "MIT)", "AND MIT", "MIT WITH"]) {
    assert.throws(() => parseSpdx(bad), undefined, bad);
  }
});

test("a GPL dependency fails the check", () => {
  const dir = project({
    "node_modules/ok": { version: "1.0.0", license: "MIT" },
    "node_modules/bad": { version: "2.0.0", license: "GPL-3.0-only" },
  });
  try {
    const { violations } = checkPackages(collectPackages(dir), policy);
    assert.deepEqual(violations, ['bad@2.0.0: license "GPL-3.0-only" is not allowed']);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("missing and non-SPDX licenses fail; manifest fallback is used", () => {
  const dir = project({
    "node_modules/nolicense": { version: "1.0.0" },
    "node_modules/custom": { version: "1.0.0", license: "SEE LICENSE IN LICENSE.txt" },
    "node_modules/fromdisk": { version: "1.0.0" },
    "node_modules/a/node_modules/@scope/legacy": { version: "1.0.0" },
  });
  mkdirSync(join(dir, "node_modules/fromdisk"), { recursive: true });
  writeFileSync(join(dir, "node_modules/fromdisk/package.json"), JSON.stringify({ license: "ISC" }));
  mkdirSync(join(dir, "node_modules/a/node_modules/@scope/legacy"), { recursive: true });
  writeFileSync(
    join(dir, "node_modules/a/node_modules/@scope/legacy/package.json"),
    JSON.stringify({ licenses: [{ type: "MIT" }, { type: "Apache-2.0" }] }),
  );
  try {
    const packages = collectPackages(dir);
    assert.equal(packages.find((p) => p.name === "@scope/legacy").license, "(MIT OR Apache-2.0)");
    const { violations } = checkPackages(packages, policy);
    assert.deepEqual(violations.sort(), [
      'custom@1.0.0: license "SEE LICENSE IN LICENSE.txt" is not a valid SPDX expression',
      "nolicense@1.0.0: no license declared",
    ]);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("dev-only exception passes for dev packages and fails at runtime", () => {
  const dir = project({
    "node_modules/css-tool-linux": { version: "1.0.0", license: "MPL-2.0", dev: true },
    "node_modules/css-tool-win": { version: "1.0.0", license: "MPL-2.0", devOptional: true },
    "node_modules/other": { version: "1.0.0", license: "MPL-2.0", dev: true },
  });
  try {
    const { violations, unusedExceptions } = checkPackages(collectPackages(dir), policy);
    assert.equal(unusedExceptions.length, 0);
    assert.equal(violations.length, 2);
    assert.match(violations.find((v) => v.startsWith("css-tool-win")), /only allowed as a build-time/);
    assert.match(violations.find((v) => v.startsWith("other")), /is not allowed/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("unused exceptions are reported", () => {
  const { violations, unusedExceptions } = checkPackages([{ name: "x", version: "1", license: "MIT" }], policy);
  assert.equal(violations.length, 0);
  assert.equal(unusedExceptions.length, 1);
});

test("exceptions without a reason are rejected", () => {
  assert.throws(() => checkPackages([], { allow: [], exceptions: [{ package: "x", license: "MPL-2.0" }] }));
});

test("CLI exits 1 on a violation and 0 when clean", () => {
  const bad = project({ "node_modules/bad": { version: "1.0.0", license: "AGPL-3.0-only" } });
  const good = project({ "node_modules/ok": { version: "1.0.0", license: "MIT" } });
  try {
    assert.equal(spawnSync(process.execPath, [SCRIPT, bad]).status, 1);
    assert.equal(spawnSync(process.execPath, [SCRIPT, good]).status, 0);
    assert.equal(spawnSync(process.execPath, [SCRIPT, join(bad, "missing")]).status, 2);
  } finally {
    rmSync(bad, { recursive: true, force: true });
    rmSync(good, { recursive: true, force: true });
  }
});
