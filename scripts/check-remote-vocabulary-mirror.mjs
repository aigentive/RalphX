#!/usr/bin/env node

/**
 * Mirrors the protocol crate's wire VOCABULARY — transport error codes and reset reasons —
 * into a checked-in TS module, the same way the local-only event and agent-control command
 * mirrors work.
 *
 * Why this exists: both vocabularies were hand-maintained on the client with only a length
 * pin behind them. An 11th Rust error code degraded silently (`parseRemoteTransportErrorCode`
 * returns null, so a transport failure surfaced as the command's own `Err` and classified
 * transient), and a new reset reason made the client reject the whole frame as malformed.
 *
 * FAIL CLOSED: a missing/unparseable snapshot, or a vocabulary array that is absent, empty,
 * or not all strings, throws rather than emitting a smaller model. There is no
 * `--allow-incomplete` escape hatch.
 *
 * Usage:
 *   node scripts/check-remote-vocabulary-mirror.mjs [repoRoot] [--update]
 */

import fs from "node:fs";
import path from "node:path";

const args = process.argv.slice(2);
const update = args.includes("--update");
const positional = args.find((arg) => !arg.startsWith("--"));
const repoRoot = path.resolve(positional ?? process.cwd());
const snapshotPath = path.join(
  repoRoot,
  "src-tauri/crates/ralphx-remote-protocol/tests/snapshots/vocabulary.json"
);
const outputPath = path.join(
  repoRoot,
  "frontend/src/lib/remote/remote-vocabulary.generated.ts"
);

function readVocabulary(snapshot, key) {
  const value = snapshot[key];
  if (!Array.isArray(value) || value.length === 0) {
    throw new Error(`vocabulary snapshot has no non-empty \`${key}\` array`);
  }
  if (!value.every((entry) => typeof entry === "string" && entry.length > 0)) {
    throw new Error(`vocabulary snapshot \`${key}\` contains a non-string entry`);
  }
  return value;
}

if (!fs.existsSync(snapshotPath)) {
  console.error(`protocol vocabulary snapshot is missing at ${snapshotPath}`);
  process.exit(1);
}

let snapshot;
try {
  snapshot = JSON.parse(fs.readFileSync(snapshotPath, "utf8"));
} catch (error) {
  console.error(`protocol vocabulary snapshot is unparseable: ${error.message}`);
  process.exit(1);
}

let errorCodes;
let resetReasons;
try {
  errorCodes = readVocabulary(snapshot, "errorCodes");
  resetReasons = readVocabulary(snapshot, "resetReasons");
} catch (error) {
  console.error(error.message);
  process.exit(1);
}

const list = (values) => values.map((value) => `  ${JSON.stringify(value)},`).join("\n");

const expected = `// GENERATED — do not edit; run node scripts/check-remote-vocabulary-mirror.mjs --update
//
// Source: src-tauri/crates/ralphx-remote-protocol/tests/snapshots/vocabulary.json

export const PROTOCOL_TRANSPORT_ERROR_CODES = [
${list(errorCodes)}
] as const;

export const PROTOCOL_RESET_REASONS = [
${list(resetReasons)}
] as const;
`;

if (update) {
  fs.writeFileSync(outputPath, expected);
  console.log(
    `Updated ${path.relative(repoRoot, outputPath)} ` +
      `(${errorCodes.length} error codes, ${resetReasons.length} reset reasons).`
  );
  process.exit(0);
}

const actual = fs.existsSync(outputPath) ? fs.readFileSync(outputPath, "utf8") : "";
if (actual !== expected) {
  console.error(
    "Remote protocol vocabulary mirror is stale. Run: " +
      "node scripts/check-remote-vocabulary-mirror.mjs --update"
  );
  process.exit(1);
}
console.log(
  `Remote protocol vocabulary mirror is current ` +
    `(${errorCodes.length} error codes, ${resetReasons.length} reset reasons).`
);
