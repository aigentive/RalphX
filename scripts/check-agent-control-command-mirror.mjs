#!/usr/bin/env node

/**
 * Mirrors the `ui:agent`-gated command set from `docs/generated/remote-commands.json`
 * into a checked-in TS module the client can import (PR 2.6-b, Decision 1).
 *
 * Why a generated mirror rather than a JSON import: the runtime bundle stays free of
 * a 540-row manifest, no Vite alias has to reach outside `frontend/`, and vitest and
 * the production build see byte-identical input. Drift is caught the same way the
 * local-only event mirror catches it — a CI-shaped script plus a test that re-derives
 * from the manifest and compares.
 *
 * FAIL CLOSED (contract constraint 6). The derivation in
 * `scripts/lib/agent-control-derivation.mjs` throws rather than emitting a smaller
 * set, because "empty list" downstream reads as "nothing is gated": manifest missing
 * or unparseable, any `coverage.*` flag not `"complete"`, or an empty derived union.
 * There is deliberately NO `--allow-incomplete` escape hatch.
 *
 * Usage:
 *   node scripts/check-agent-control-command-mirror.mjs [repoRoot]
 *   node scripts/check-agent-control-command-mirror.mjs [repoRoot] --update
 */

import fs from "node:fs";
import path from "node:path";

import { deriveGatedCommands } from "./lib/agent-control-derivation.mjs";

const args = process.argv.slice(2);
const update = args.includes("--update");
const positional = args.find((arg) => !arg.startsWith("--"));
const repoRoot = path.resolve(positional ?? process.cwd());

const manifestPath = path.join(repoRoot, "docs/generated/remote-commands.json");
const outputPath = path.join(
  repoRoot,
  "frontend/src/lib/remote/agent-control-commands.generated.ts"
);

function readManifest() {
  if (!fs.existsSync(manifestPath)) {
    throw new Error(
      `remote-commands manifest is missing at ${manifestPath}; ` +
        "the ui:agent gate set cannot be derived"
    );
  }
  return JSON.parse(fs.readFileSync(manifestPath, "utf8"));
}

function render(commands, schemaVersion) {
  const quoted = commands.map((name) => `  ${JSON.stringify(name)},`).join("\n");
  return `// GENERATED — do not edit; run node scripts/check-agent-control-command-mirror.mjs --update

/** \`schemaVersion\` of the manifest this mirror was generated from. */
export const AGENT_CONTROL_MANIFEST_SCHEMA_VERSION = ${schemaVersion};

/**
 * Commands that require the \`ui:agent\` scope: every \`class: "agentControl"\` ledger
 * row, unioned with \`agent_control_floor\` and \`declared_memberships\`. See
 * \`scripts/lib/agent-control-derivation.mjs\` for why all three sources are needed.
 */
export const AGENT_CONTROL_COMMANDS = [
${quoted}
] as const;

export const AGENT_CONTROL_COMMAND_NAMES: ReadonlySet<string> = new Set(
  AGENT_CONTROL_COMMANDS
);
`;
}

const manifest = readManifest();
const commands = deriveGatedCommands(manifest);
const schemaVersion =
  typeof manifest.schemaVersion === "number" ? manifest.schemaVersion : 0;
const expected = render(commands, schemaVersion);

if (update) {
  fs.mkdirSync(path.dirname(outputPath), { recursive: true });
  fs.writeFileSync(outputPath, expected);
  console.log(
    `Updated ${path.relative(repoRoot, outputPath)} (${commands.length} commands).`
  );
  process.exit(0);
}

const actual = fs.existsSync(outputPath) ? fs.readFileSync(outputPath, "utf8") : "";
if (actual !== expected) {
  console.error(
    "Agent-control command mirror is stale. Run: " +
      "node scripts/check-agent-control-command-mirror.mjs --update"
  );
  process.exit(1);
}
console.log(`Agent-control command mirror is current (${commands.length} commands).`);
