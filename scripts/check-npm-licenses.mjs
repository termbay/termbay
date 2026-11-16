#!/usr/bin/env node
// Checks the licenses of every npm package in a package-lock.json against
// scripts/npm-license-policy.json. No dependencies: only Node built-ins.
//
// Usage (from ui/):   node ../scripts/check-npm-licenses.mjs
// Or with a path:     node scripts/check-npm-licenses.mjs ui
//
// It reads the lockfile, so it covers every package npm could install on any
// platform (including optional native binaries for other systems). When the
// lockfile has no license for a package, it falls back to
// node_modules/<pkg>/package.json. A package with no license is a failure.
//
// Exit code: 0 when everything is allowed, 1 on any violation, 2 on usage errors.

import { readFileSync, existsSync } from "node:fs";
import { join, resolve, dirname } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const DEFAULT_POLICY = join(dirname(fileURLToPath(import.meta.url)), "npm-license-policy.json");

// ---------------------------------------------------------------------------
// SPDX expressions: a small parser for `A OR B`, `A AND B`, `A WITH E` and
// parentheses. Precedence: WITH > AND > OR (as in the SPDX spec).
// ---------------------------------------------------------------------------

export function parseSpdx(expression) {
  const tokens = String(expression)
    .replace(/[()]/g, (p) => ` ${p} `)
    .trim()
    .split(/\s+/)
    .filter(Boolean);
  let pos = 0;
  const peek = () => tokens[pos];
  const next = () => tokens[pos++];

  function parseOr() {
    let node = parseAnd();
    while (peek() && peek().toUpperCase() === "OR") {
      next();
      node = { op: "OR", left: node, right: parseAnd() };
    }
    return node;
  }
  function parseAnd() {
    let node = parseWith();
    while (peek() && peek().toUpperCase() === "AND") {
      next();
      node = { op: "AND", left: node, right: parseWith() };
    }
    return node;
  }
  function parseWith() {
    const node = parseAtom();
    if (peek() && peek().toUpperCase() === "WITH") {
      next();
      const exception = next();
      if (!exception || /^(\(|\)|AND|OR|WITH)$/i.test(exception)) {
        throw new Error(`invalid SPDX expression: ${expression}`);
      }
      return { id: `${node.id} WITH ${exception}` };
    }
    return node;
  }
  function parseAtom() {
    const token = next();
    if (token === "(") {
      const node = parseOr();
      if (next() !== ")") throw new Error(`invalid SPDX expression: ${expression}`);
      return node;
    }
    if (!token || /^(\)|AND|OR|WITH)$/i.test(token)) {
      throw new Error(`invalid SPDX expression: ${expression}`);
    }
    return { id: token };
  }

  if (tokens.length === 0) throw new Error("empty SPDX expression");
  const tree = parseOr();
  if (pos !== tokens.length) throw new Error(`invalid SPDX expression: ${expression}`);
  return tree;
}

// Returns true when the expression can be satisfied with licenses for which
// `isAllowed(id)` is true.
export function satisfies(tree, isAllowed) {
  if (tree.id) return isAllowed(tree.id);
  if (tree.op === "OR") return satisfies(tree.left, isAllowed) || satisfies(tree.right, isAllowed);
  return satisfies(tree.left, isAllowed) && satisfies(tree.right, isAllowed);
}

// ---------------------------------------------------------------------------
// Reading packages
// ---------------------------------------------------------------------------

// Old package.json style: "license": { "type": "MIT" } or "licenses": [{ "type": "MIT" }, ...].
function licenseFromManifest(manifest) {
  if (!manifest) return undefined;
  const { license, licenses } = manifest;
  if (typeof license === "string") return license;
  if (license && typeof license.type === "string") return license.type;
  if (Array.isArray(licenses) && licenses.length > 0) {
    const ids = licenses.map((l) => (typeof l === "string" ? l : l && l.type)).filter(Boolean);
    if (ids.length > 0) return ids.length === 1 ? ids[0] : `(${ids.join(" OR ")})`;
  }
  return undefined;
}

function readJson(path) {
  return JSON.parse(readFileSync(path, "utf8"));
}

// Lists { name, version, path, license, dev } for every package in the lockfile.
export function collectPackages(projectDir) {
  const lockPath = join(projectDir, "package-lock.json");
  if (!existsSync(lockPath)) {
    throw new Error(`package-lock.json not found in ${projectDir}`);
  }
  const lock = readJson(lockPath);
  if (!lock.packages) {
    throw new Error("package-lock.json uses lockfileVersion 1; run npm install with npm 7 or newer");
  }
  const result = [];
  for (const [path, entry] of Object.entries(lock.packages)) {
    if (path === "" || entry.link) continue; // the project itself, or a workspace link
    const name = entry.name ?? path.slice(path.lastIndexOf("node_modules/") + "node_modules/".length);
    let license = licenseFromManifest(entry);
    if (license === undefined) {
      const manifestPath = join(projectDir, path, "package.json");
      if (existsSync(manifestPath)) license = licenseFromManifest(readJson(manifestPath));
    }
    // `dev: true` means only devDependencies need it. `devOptional` means it is
    // also an optional production dependency, so it does not count as dev-only.
    result.push({ name, version: entry.version, path, license, dev: entry.dev === true });
  }
  return result;
}

// ---------------------------------------------------------------------------
// Policy check
// ---------------------------------------------------------------------------

function matchesPackage(pattern, name) {
  return pattern.endsWith("*") ? name.startsWith(pattern.slice(0, -1)) : pattern === name;
}

export function checkPackages(packages, policy) {
  const allow = new Set(policy.allow ?? []);
  const exceptions = policy.exceptions ?? [];
  for (const ex of exceptions) {
    if (!ex.package || !ex.license || !ex.reason) {
      throw new Error(`policy exception needs "package", "license" and "reason": ${JSON.stringify(ex)}`);
    }
  }
  const usedExceptions = new Set();
  const violations = [];
  const byLicense = new Map();

  for (const pkg of packages) {
    const label = `${pkg.name}@${pkg.version ?? "?"}`;
    if (!pkg.license) {
      violations.push(`${label}: no license declared`);
      continue;
    }
    byLicense.set(pkg.license, (byLicense.get(pkg.license) ?? 0) + 1);

    let tree;
    try {
      tree = parseSpdx(pkg.license);
    } catch {
      violations.push(`${label}: license "${pkg.license}" is not a valid SPDX expression`);
      continue;
    }

    if (satisfies(tree, (id) => allow.has(id))) continue;

    // Not allowed by the main list: try exceptions for this package.
    const applicable = exceptions.filter((ex) => matchesPackage(ex.package, pkg.name));
    const okWith = (ex) => satisfies(tree, (id) => allow.has(id) || id === ex.license);
    const match = applicable.find(okWith);
    if (!match) {
      violations.push(`${label}: license "${pkg.license}" is not allowed`);
      continue;
    }
    usedExceptions.add(match);
    if (match.devOnly && !pkg.dev) {
      violations.push(
        `${label}: "${pkg.license}" is only allowed as a build-time (dev) dependency, but it is now a runtime dependency`,
      );
    }
  }

  const unusedExceptions = exceptions.filter((ex) => !usedExceptions.has(ex));
  return { violations, unusedExceptions, byLicense, total: packages.length };
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

function main(argv) {
  const args = argv.slice(2);
  let policyPath = DEFAULT_POLICY;
  const rest = [];
  for (let i = 0; i < args.length; i++) {
    if (args[i] === "--policy") policyPath = args[++i];
    else if (args[i] === "--help" || args[i] === "-h") {
      console.log("usage: check-npm-licenses.mjs [project-dir] [--policy file.json]");
      return 0;
    } else rest.push(args[i]);
  }
  if (rest.length > 1 || !policyPath) {
    console.error("usage: check-npm-licenses.mjs [project-dir] [--policy file.json]");
    return 2;
  }
  const projectDir = resolve(rest[0] ?? process.cwd());

  let packages, policy;
  try {
    policy = readJson(policyPath);
    packages = collectPackages(projectDir);
  } catch (err) {
    console.error(`license check: ${err.message}`);
    return 2;
  }

  const { violations, unusedExceptions, byLicense, total } = checkPackages(packages, policy);

  console.log(`Checked ${total} npm packages:`);
  for (const [license, count] of [...byLicense].sort((a, b) => b[1] - a[1])) {
    console.log(`  ${String(count).padStart(4)}  ${license}`);
  }
  for (const ex of unusedExceptions) {
    console.warn(`warning: exception for "${ex.package}" (${ex.license}) is no longer used; remove it from the policy`);
  }
  if (violations.length > 0) {
    console.error(`\n${violations.length} license problem(s):`);
    for (const v of violations) console.error(`  - ${v}`);
    console.error("\nSee CONTRIBUTING.md, section 'Licenças das dependências'.");
    return 1;
  }
  console.log("All npm licenses are allowed.");
  return 0;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  process.exitCode = main(process.argv);
}
