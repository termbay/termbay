#!/usr/bin/env node
// Measures how long the termbay binary takes to start and exit (task F1 in
// docs/FASE2-PLANO.md). Claude Code spawns `termbay hook` once per event, so
// process startup is the hook's floor. The cost that matters is paid before
// main(): the dynamic loader maps every linked library (WebKitGTK and GTK on
// Linux), whatever the subcommand, so `--version` measures it today.
//
// Usage:
//   node scripts/measure-startup.mjs <binary> [--runs 50] [--warmup 3]
//        [--budget-ms N] [--title "..."] [-- args passed to the binary]
//
// Without --budget-ms it only reports. With it, it fails when p95 is over.
// Writes a Markdown table to stdout and, on GitHub Actions, to the job summary.

import { spawnSync } from "node:child_process";
import { appendFileSync, existsSync } from "node:fs";
import { pathToFileURL } from "node:url";

/** p-th percentile (0..100) of `values`, nearest-rank, on a sorted copy. */
export function percentile(values, p) {
  if (values.length === 0) throw new Error("no values");
  const sorted = [...values].sort((a, b) => a - b);
  const rank = Math.ceil((p / 100) * sorted.length);
  return sorted[Math.min(sorted.length, Math.max(1, rank)) - 1];
}

/** Summary of a set of timings in milliseconds. */
export function summarize(timings) {
  return {
    runs: timings.length,
    p50: percentile(timings, 50),
    p95: percentile(timings, 95),
    max: Math.max(...timings),
  };
}

export function parseArgs(argv) {
  const options = { runs: 50, warmup: 3, budgetMs: null, title: "Startup time", binary: null, binaryArgs: ["--version"] };
  const number = (flag, value) => {
    const n = Number(value);
    if (!Number.isFinite(n) || n < 0) throw new Error(`${flag} needs a non-negative number, got ${value}`);
    return n;
  };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === "--") {
      options.binaryArgs = argv.slice(i + 1);
      break;
    } else if (arg === "--runs") options.runs = number(arg, argv[++i]);
    else if (arg === "--warmup") options.warmup = number(arg, argv[++i]);
    else if (arg === "--budget-ms") options.budgetMs = number(arg, argv[++i]);
    else if (arg === "--title") options.title = argv[++i];
    else if (arg.startsWith("-")) throw new Error(`unknown option ${arg}`);
    else if (options.binary === null) options.binary = arg;
    else throw new Error(`unexpected argument ${arg}`);
  }
  if (options.binary === null) throw new Error("missing <binary>");
  if (options.runs < 1) throw new Error("--runs must be at least 1");
  return options;
}

/** Runs the binary once and returns the wall time in ms; throws if it fails. */
function timeOnce(binary, args) {
  const start = process.hrtime.bigint();
  const result = spawnSync(binary, args, { stdio: "ignore" });
  const elapsed = Number(process.hrtime.bigint() - start) / 1e6;
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${binary} ${args.join(" ")} exited with ${result.status ?? result.signal}`);
  return elapsed;
}

/** Linked libraries on Linux, to tell how much the loader has to map. */
function linkedLibraries(binary) {
  if (process.platform !== "linux") return null;
  const ldd = spawnSync("ldd", [binary], { encoding: "utf8" });
  if (ldd.status !== 0) return null;
  const lines = ldd.stdout.split("\n").filter((l) => l.includes("=>"));
  return { count: lines.length, webkit: lines.some((l) => /webkit/i.test(l)) };
}

export function report(options, first, stats, libs) {
  const ms = (n) => `${n.toFixed(1)} ms`;
  const rows = [
    `### ${options.title}`,
    "",
    `\`${options.binary} ${options.binaryArgs.join(" ")}\` on ${process.platform}/${process.arch}, ${stats.runs} runs after ${options.warmup} warm-up`,
    "",
    "| first run (cold) | p50 | p95 | max | budget (p95) |",
    "|---|---|---|---|---|",
    `| ${ms(first)} | ${ms(stats.p50)} | ${ms(stats.p95)} | ${ms(stats.max)} | ${options.budgetMs === null ? "report only" : ms(options.budgetMs)} |`,
  ];
  if (libs) rows.push("", `Linked libraries: ${libs.count}${libs.webkit ? ", WebKitGTK among them" : ""}.`);
  return rows.join("\n") + "\n";
}

function main() {
  const options = parseArgs(process.argv.slice(2));
  if (!existsSync(options.binary)) throw new Error(`${options.binary} does not exist`);

  const first = timeOnce(options.binary, options.binaryArgs);
  for (let i = 0; i < options.warmup; i++) timeOnce(options.binary, options.binaryArgs);
  const timings = [];
  for (let i = 0; i < options.runs; i++) timings.push(timeOnce(options.binary, options.binaryArgs));
  const stats = summarize(timings);

  const text = report(options, first, stats, linkedLibraries(options.binary));
  process.stdout.write(text);
  if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, text + "\n");

  if (options.budgetMs !== null && stats.p95 > options.budgetMs) {
    console.error(`error: p95 ${stats.p95.toFixed(1)} ms is over the ${options.budgetMs} ms budget`);
    process.exit(1);
  }
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  try {
    main();
  } catch (error) {
    console.error(`error: ${error.message}`);
    process.exit(2);
  }
}
