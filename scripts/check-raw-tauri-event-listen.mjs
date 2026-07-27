#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";

const repoRoot = path.resolve(process.argv[2] ?? process.cwd());
const sourceRoot = path.join(repoRoot, "frontend", "src");

const RAW_LISTEN_ALLOWLIST = new Set([
  "frontend/src/lib/event-bus.ts",
]);
const EXCLUDED_DIRECTORIES = new Set([
  "frontend/src/mocks",
  "frontend/src/test",
]);
const TEST_FILE_PATTERN = /\.(?:test|spec)\.[cm]?[jt]sx?$/;
const SOURCE_FILE_PATTERN = /\.[cm]?[jt]sx?$/;

function toRepoPath(filePath) {
  return path.relative(repoRoot, filePath).split(path.sep).join("/");
}

function isExcluded(filePath) {
  const repoPath = toRepoPath(filePath);
  return (
    TEST_FILE_PATTERN.test(repoPath) ||
    [...EXCLUDED_DIRECTORIES].some(
      (directory) => repoPath === directory || repoPath.startsWith(`${directory}/`),
    )
  );
}

function sourceFiles(directory) {
  return fs.readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const entryPath = path.join(directory, entry.name);
    if (entry.isDirectory()) {
      return isExcluded(entryPath) ? [] : sourceFiles(entryPath);
    }
    return entry.isFile() && SOURCE_FILE_PATTERN.test(entry.name) && !isExcluded(entryPath)
      ? [entryPath]
      : [];
  });
}

function rawListenImports(source) {
  const importPattern = /import\s+([\s\S]*?)\s+from\s+["']@tauri-apps\/api\/event["']/g;
  return [...source.matchAll(importPattern)].filter((match) => {
    const bindings = match[1] ?? "";
    return /(?:^|[,{\s])listen(?:\s+as\s+[A-Za-z_$][\w$]*)?(?=\s*[,}])/.test(bindings)
      || /^\s*\*\s+as\s+/.test(bindings);
  });
}

if (!fs.existsSync(sourceRoot)) {
  console.error(`FAIL: missing frontend source directory: ${toRepoPath(sourceRoot)}`);
  process.exit(1);
}

const violations = [];
for (const filePath of sourceFiles(sourceRoot)) {
  const repoPath = toRepoPath(filePath);
  if (RAW_LISTEN_ALLOWLIST.has(repoPath)) {
    continue;
  }

  const source = fs.readFileSync(filePath, "utf8");
  for (const match of rawListenImports(source)) {
    const line = source.slice(0, match.index).split("\n").length;
    violations.push(`${repoPath}:${line}`);
  }
}

if (violations.length > 0) {
  console.error("FAIL: raw Tauri event listen imports must go through useEventBus().");
  console.error(`Allowed raw-listen module: ${[...RAW_LISTEN_ALLOWLIST].join(", ")}`);
  violations.forEach((violation) => console.error(`  ${violation}`));
  process.exit(1);
}

console.log("PASS: raw Tauri event listen is confined to frontend/src/lib/event-bus.ts");
