#!/usr/bin/env node

/**
 * Mirrors the remote capability model from `docs/generated/remote-commands.json`
 * into a checked-in TS module the client can import (PR 2.6-b, Decision 1).
 *
 * Why a generated mirror rather than a JSON import: the runtime bundle stays free of
 * the full manifest, no Vite alias has to reach outside `frontend/`, and vitest and
 * the production build see byte-identical input. Drift is caught the same way the
 * local-only event mirror catches it — a CI-shaped script plus a test that re-derives
 * from the manifest and compares.
 *
 * FAIL CLOSED (contract constraint 6). The derivation in
 * `scripts/lib/agent-control-derivation.mjs` throws rather than emitting a smaller
 * model: missing/unparseable manifest, incomplete `coverage.*`, an UNKNOWN
 * `schemaVersion`, a malformed facade op, or an empty op set. There is deliberately
 * NO `--allow-incomplete` escape hatch.
 *
 * Usage:
 *   node scripts/check-agent-control-command-mirror.mjs [repoRoot]
 *   node scripts/check-agent-control-command-mirror.mjs [repoRoot] --update
 */

import fs from "node:fs";
import path from "node:path";

import { deriveRemoteCapabilityModel } from "./lib/agent-control-derivation.mjs";

const args = process.argv.slice(2);
const update = args.includes("--update");
const positional = args.find((arg) => !arg.startsWith("--"));
const repoRoot = path.resolve(positional ?? process.cwd());

const manifestPath = path.join(repoRoot, "docs/generated/remote-commands.json");
const outputPath = path.join(
  repoRoot,
  "frontend/src/lib/remote/remote-capabilities.generated.ts"
);

function readManifest() {
  if (!fs.existsSync(manifestPath)) {
    throw new Error(
      `remote-commands manifest is missing at ${manifestPath}; ` +
        "the remote capability model cannot be derived"
    );
  }
  return JSON.parse(fs.readFileSync(manifestPath, "utf8"));
}

function render({ schemaVersion, ops, conditionals }) {
  const opRows = ops
    .map(
      (op) =>
        `  ${JSON.stringify(op.command)}: {\n` +
        `    opClass: ${JSON.stringify(op.opClass)},\n` +
        `    argumentSensitive: ${op.argumentSensitive},\n` +
        `    capabilities: ${JSON.stringify(op.capabilities)},\n` +
        `    pins: ${JSON.stringify(op.pins)},\n` +
        `  },`
    )
    .join("\n");

  const conditionalRows = conditionals
    .map(
      (row) =>
        `  ${JSON.stringify(row.command)}: {\n` +
        `    capability: ${JSON.stringify(row.capability)},\n` +
        `    fields: ${JSON.stringify(row.fields)},\n` +
        `  },`
    )
    .join("\n");

  return `// GENERATED — do not edit; run node scripts/check-agent-control-command-mirror.mjs --update

/** \`schemaVersion\` of the manifest this mirror was generated from. */
export const REMOTE_MANIFEST_SCHEMA_VERSION = ${schemaVersion};

/** Scope class a remotely-reachable operation is served under. */
export type RemoteOpClass = "read" | "operate" | "agentControl";

export interface RemoteFacadePin {
  readonly param: string;
  readonly field: string;
  readonly value: unknown;
}

export interface RemoteFacadeOp {
  readonly opClass: RemoteOpClass;
  /** The host inspects arguments to decide the effective class (see conditionals). */
  readonly argumentSensitive: boolean;
  readonly capabilities: readonly string[];
  /** Argument values the facade pins, e.g. \`decision: "deny"\`. */
  readonly pins: readonly RemoteFacadePin[];
}

/**
 * Every operation the host facade exposes remotely. A command ABSENT from this map is
 * not reachable remotely at all — no scope grant changes that.
 */
export const REMOTE_FACADE_OPS: Readonly<Record<string, RemoteFacadeOp>> = {
${opRows}
};

export interface RemoteConditionalCapability {
  readonly capability: string;
  /** Argument fields that escalate the op's effective class to \`agentControl\`. */
  readonly fields: readonly string[];
}

/** Ops whose required scope depends on WHICH fields the caller is changing. */
export const REMOTE_CONDITIONAL_CAPABILITIES: Readonly<
  Record<string, RemoteConditionalCapability>
> = {
${conditionalRows}
};
`;
}

const manifest = readManifest();
const model = deriveRemoteCapabilityModel(manifest);
const expected = render(model);

if (update) {
  fs.mkdirSync(path.dirname(outputPath), { recursive: true });
  fs.writeFileSync(outputPath, expected);
  console.log(
    `Updated ${path.relative(repoRoot, outputPath)} (${model.ops.length} facade ops, ` +
      `${model.conditionals.length} conditional).`
  );
  process.exit(0);
}

const actual = fs.existsSync(outputPath) ? fs.readFileSync(outputPath, "utf8") : "";
if (actual !== expected) {
  console.error(
    "Remote capability mirror is stale. Run: " +
      "node scripts/check-agent-control-command-mirror.mjs --update"
  );
  process.exit(1);
}
console.log(`Remote capability mirror is current (${model.ops.length} facade ops).`);
