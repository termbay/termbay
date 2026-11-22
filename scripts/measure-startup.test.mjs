import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { parseArgs, percentile, report, summarize } from "./measure-startup.mjs";

const script = fileURLToPath(new URL("./measure-startup.mjs", import.meta.url));

test("percentile uses the nearest rank on a sorted copy", () => {
  const values = [5, 1, 4, 2, 3];
  assert.equal(percentile(values, 50), 3);
  assert.equal(percentile(values, 95), 5);
  assert.equal(percentile(values, 0), 1);
  assert.equal(percentile([7], 95), 7);
  assert.deepEqual(values, [5, 1, 4, 2, 3], "input is not reordered");
  assert.throws(() => percentile([], 50), /no values/);
});

test("summarize reports runs, p50, p95 and max", () => {
  const timings = Array.from({ length: 20 }, (_, i) => i + 1);
  assert.deepEqual(summarize(timings), { runs: 20, p50: 10, p95: 19, max: 20 });
});

test("parseArgs has defaults and passes everything after -- to the binary", () => {
  assert.deepEqual(parseArgs(["bin"]), {
    runs: 50, warmup: 3, budgetMs: null, title: "Startup time", binary: "bin", binaryArgs: ["--version"],
  });
  const o = parseArgs(["--runs", "10", "--budget-ms", "30", "bin", "--", "hook", "PreToolUse"]);
  assert.equal(o.runs, 10);
  assert.equal(o.budgetMs, 30);
  assert.deepEqual(o.binaryArgs, ["hook", "PreToolUse"]);
});

test("parseArgs rejects bad input", () => {
  assert.throws(() => parseArgs([]), /missing <binary>/);
  assert.throws(() => parseArgs(["--runs", "x", "bin"]), /non-negative number/);
  assert.throws(() => parseArgs(["--runs", "0", "bin"]), /at least 1/);
  assert.throws(() => parseArgs(["--nope", "bin"]), /unknown option/);
  assert.throws(() => parseArgs(["a", "b"]), /unexpected argument/);
});

test("report says when there is no budget and mentions WebKitGTK", () => {
  const options = parseArgs(["bin"]);
  const text = report(options, 12.34, { runs: 50, p50: 3, p95: 4.5, max: 9 }, { count: 80, webkit: true });
  assert.match(text, /\| 12\.3 ms \| 3\.0 ms \| 4\.5 ms \| 9\.0 ms \| report only \|/);
  assert.match(text, /Linked libraries: 80, WebKitGTK among them\./);
});

test("the CLI measures a real process and enforces the budget", () => {
  const node = process.execPath;
  const ok = spawnSync(node, [script, node, "--runs", "3", "--warmup", "0", "--", "-e", ""], { encoding: "utf8" });
  assert.equal(ok.status, 0, ok.stderr);
  assert.match(ok.stdout, /report only/);

  const over = spawnSync(node, [script, node, "--runs", "2", "--warmup", "0", "--budget-ms", "0", "--", "-e", ""], { encoding: "utf8" });
  assert.equal(over.status, 1);
  assert.match(over.stderr, /over the 0 ms budget/);

  const failing = spawnSync(node, [script, node, "--runs", "1", "--", "-e", "process.exit(3)"], { encoding: "utf8" });
  assert.equal(failing.status, 2);
  assert.match(failing.stderr, /exited with 3/);
});
