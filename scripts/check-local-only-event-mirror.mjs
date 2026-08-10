#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";

const args = process.argv.slice(2);
const update = args.includes("--update");
const positional = args.find((arg) => !arg.startsWith("--"));
const repoRoot = path.resolve(positional ?? process.cwd());
const snapshotPath = path.join(
  repoRoot,
  "src-tauri/crates/ralphx-remote-protocol/tests/snapshots/event-classifications.json"
);
const outputPath = path.join(
  repoRoot,
  "frontend/src/lib/remote/local-only-backend-events.generated.ts"
);

const classifications = JSON.parse(fs.readFileSync(snapshotPath, "utf8"));
if (!Array.isArray(classifications)) {
  throw new Error(`Expected an array in ${snapshotPath}`);
}

const names = classifications
  .filter((row) => row.delivery === "localOnly" && row.origin === "backend")
  .map((row) => row.name)
  .sort();
const quoted = names.map((name) => `  ${JSON.stringify(name)},`).join("\n");
const expected = `// GENERATED — do not edit; run node scripts/check-local-only-event-mirror.mjs --update

export const LOCAL_ONLY_BACKEND_EVENTS = [
${quoted}
] as const;

export const LOCAL_ONLY_BACKEND_EVENT_NAMES: ReadonlySet<string> = new Set(
  LOCAL_ONLY_BACKEND_EVENTS
);
`;

if (update) {
  fs.writeFileSync(outputPath, expected);
  console.log(`Updated ${path.relative(repoRoot, outputPath)} (${names.length} events).`);
  process.exit(0);
}

const actual = fs.existsSync(outputPath) ? fs.readFileSync(outputPath, "utf8") : "";
if (actual !== expected) {
  console.error(
    "Local-only backend event mirror is stale. Run: " +
      "node scripts/check-local-only-event-mirror.mjs --update"
  );
  process.exit(1);
}
console.log(`Local-only backend event mirror is current (${names.length} events).`);
