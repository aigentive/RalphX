/**
 * Op↔callsite consistency guard (Phase 0 of the remote-coverage handoff).
 *
 * The wiring guard next door proves a file IMPORTS `useAgentGate`. That is not the same
 * as proving the gate answers the question the button asks. Two confirmed criticals
 * lived inside files that guard certified:
 *
 * - `AgentsAutomationPanel` gates "Run now" with `automationResume` → `resume_automation`
 *   (registered), while the click invokes `trigger_automation_run_now` (unregistered).
 *   The correct row `automationRunNow` exists and has zero consumers.
 * - `PlanEditor` gates on `artifactEdit` → `update_artifact` (registered), while its save
 *   POSTs `update_plan_artifact` (not on the remount allowlist).
 *
 * This file adds the two checks that can see those:
 *
 *   (a) every `AGENT_GATED_AFFORDANCES` row has at least one production consumer, and
 *   (b) every file that resolves an affordance actually reaches that row's facade op.
 *
 * Plus the never-invoke-the-raw-twins ratchet: the facade splits
 * `resolve_permission_request` into pinned approve/deny ops and denies
 * `resolve_user_question` in favour of `answer_user_question`, so a production `invoke(`
 * of either raw name is a defect by construction.
 *
 * ## Resolution is deliberately approximate
 *
 * "Reaches" is computed by an import-scoped, depth-bounded walk over source text: the
 * command literals in a file, plus those in the top-level declarations it imports,
 * transitively to `REACH_DEPTH`. It is not a call graph — it over-approximates within a
 * file (any command any imported symbol can reach counts) and under-approximates across
 * callback props (a dispatch handed in as `onSend` is invisible). The first direction
 * costs precision; the second is covered by `GATE_CALLSITE_INDIRECTIONS`, where every
 * blind spot is written down with the prop it escapes through.
 *
 * ## Quarantine
 *
 * Today's real defects live in `KNOWN_GATE_GAPS` (see that file for the shrink
 * procedure). Each check below asserts BOTH that nothing outside the quarantine fails
 * AND that everything inside it still does — so a fix that leaves its row behind turns
 * the suite red.
 */

import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import { describe, expect, it } from "vitest";

import {
  AGENT_GATED_AFFORDANCES,
  type AgentGatedAffordance,
} from "@/lib/remote/agent-gate";
import {
  AFFORDANCE_CONSUMPTION_ALIASES,
  GATE_CALLSITE_INDIRECTIONS,
  KNOWN_GATE_GAPS,
  quarantinedIds,
} from "./agent-gate-guard-manifest";

// ---------------------------------------------------------------------------
// Source index
// ---------------------------------------------------------------------------

const FRONTEND_ROOT = resolve(__dirname, "../../..");
const SRC_ROOT = join(FRONTEND_ROOT, "src");

/** How far the import-scoped walk follows symbols. 4 covers component → hook → api → transport. */
const REACH_DEPTH = 4;

/**
 * Everything that is not production code. Test files, harness/setup modules, hand-written
 * mocks, and generated manifests must never be able to satisfy a guard: a dead affordance
 * row whose only "consumer" is its own unit test is still dead.
 */
const NON_PRODUCTION = [
  /\.(test|spec)\.[cm]?[jt]sx?$/, // co-located unit tests
  /[/\\]__tests__[/\\]/,
  /[/\\]__mocks__[/\\]/,
  /[/\\]src[/\\]mocks[/\\]/,
  /[/\\]src[/\\]api-mock[/\\]/,
  /[/\\]src[/\\]test[/\\]/, // setup.ts, mock-data.ts, store-utils, test pages
  /testSetup/i,
  /TestFixtures|chatRenderFixtures|replayConversationFixture/,
  /\.generated\./,
  // This guard's own manifest names every affordance row and imports the gate module;
  // reading it as production would make every quarantined dead row look consumed.
  /agent-gate-guard-manifest\.ts$/,
];

function isProduction(path: string): boolean {
  return !NON_PRODUCTION.some((pattern) => pattern.test(path));
}

function walk(dir: string, out: string[] = []): string[] {
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry);
    if (statSync(path).isDirectory()) walk(path, out);
    else if (/\.tsx?$/.test(path)) out.push(path);
  }
  return out;
}

const PRODUCTION_FILES = walk(SRC_ROOT).filter(isProduction);
const SOURCE = new Map<string, string>(
  PRODUCTION_FILES.map((file) => [file, readFileSync(file, "utf8")])
);

const rel = (file: string): string => relative(FRONTEND_ROOT, file);

// ---------------------------------------------------------------------------
// Command extraction
// ---------------------------------------------------------------------------

/**
 * Every shape a command name reaches the transport through. `backendFetch` is included
 * because the remount routes are named like commands and PlanEditor's save is one of
 * them — excluding it would make that critical invisible.
 */
const INVOKE_PATTERN =
  /\b(?:invoke|typedInvoke|typedInvokeWithTransform|typedInvokeVoid|networkInvoke|backendFetch)\s*(?:<[^>()]*>)?\(\s*["'`]([A-Za-z0-9_:|./-]+)["'`]/g;

const IDENTIFIER_PATTERN = /\b([A-Za-z_$][A-Za-z0-9_$]*)\b/g;

function commandsIn(text: string): string[] {
  return [...text.matchAll(INVOKE_PATTERN)].map((match) => match[1] as string);
}

/**
 * Top-level declarations of a file, exported or not, keyed by name.
 *
 * Non-exported declarations matter: the remote halves (`startRemoteAgentConversation`,
 * `stopRemoteAgent`, …) are module-private helpers in `api/chat.ts`, and skipping them
 * would report every remote-intent affordance as mis-gated.
 */
const declarationCache = new Map<string, Map<string, string>>();

function declarations(file: string): Map<string, string> {
  const cached = declarationCache.get(file);
  if (cached !== undefined) return cached;

  const lines = (SOURCE.get(file) ?? "").split("\n");
  const starts: { line: number; name: string }[] = [];
  lines.forEach((line, index) => {
    const match = line.match(
      /^(?:export\s+)?(?:async\s+)?(?:function|const|let|class)\s+([A-Za-z0-9_$]+)/
    );
    if (match) starts.push({ line: index, name: match[1] as string });
  });

  const regions = new Map<string, string>();
  starts.forEach((start, index) => {
    const end = index + 1 < starts.length ? (starts[index + 1] as { line: number }).line : lines.length;
    regions.set(start.name, lines.slice(start.line, end).join("\n"));
  });
  declarationCache.set(file, regions);
  return regions;
}

function resolveSpecifier(fromFile: string, specifier: string): string | null {
  let base: string;
  if (specifier.startsWith("@/")) base = join(SRC_ROOT, specifier.slice(2));
  else if (specifier.startsWith(".")) base = resolve(fromFile, "..", specifier);
  else return null;

  for (const candidate of [
    `${base}.ts`,
    `${base}.tsx`,
    join(base, "index.ts"),
    join(base, "index.tsx"),
  ]) {
    if (SOURCE.has(candidate)) return candidate;
  }
  return null;
}

interface FileImports {
  /** local binding → the declaration it points at */
  readonly named: Map<string, { file: string; name: string }>;
  /** `import * as ns` targets, folded in at module granularity */
  readonly namespaces: Set<string>;
}

const IMPORT_PATTERN = /import\s+(type\s+)?([\s\S]*?)\s+from\s+["']([^"']+)["']/g;
const importCache = new Map<string, FileImports>();

function importsOf(file: string): FileImports {
  const cached = importCache.get(file);
  if (cached !== undefined) return cached;

  const named = new Map<string, { file: string; name: string }>();
  const namespaces = new Set<string>();

  for (const match of (SOURCE.get(file) ?? "").matchAll(IMPORT_PATTERN)) {
    if (match[1] !== undefined) continue; // `import type` carries no dispatch
    const target = resolveSpecifier(file, match[3] as string);
    if (target === null) continue;

    const clause = (match[2] as string).trim();
    if (clause.includes("*")) {
      namespaces.add(target);
      continue;
    }

    const braces = clause.match(/\{([\s\S]*)\}/);
    if (braces) {
      for (const raw of (braces[1] as string).split(",")) {
        const part = raw.trim();
        if (part === "" || part.startsWith("type ")) continue;
        const [original, alias] = part.split(/\s+as\s+/).map((piece) => piece.trim());
        if (original === undefined) continue;
        named.set(alias ?? original, { file: target, name: original });
      }
    }

    const defaultBinding = clause.replace(/\{[\s\S]*\}/, "").replace(/,/g, "").trim();
    if (/^[A-Za-z_$][\w$]*$/.test(defaultBinding)) {
      named.set(defaultBinding, { file: target, name: "default" });
    }
  }

  const result: FileImports = { named, namespaces };
  importCache.set(file, result);
  return result;
}

const symbolCache = new Map<string, Set<string>>();

function commandsOfSymbol(file: string, name: string, depth: number): ReadonlySet<string> {
  const key = `${file}#${name}#${depth}`;
  const cached = symbolCache.get(key);
  if (cached !== undefined) return cached;
  symbolCache.set(key, new Set()); // cycle guard: recursion sees an empty set

  const region = declarations(file).get(name);
  const found = new Set<string>();
  if (region !== undefined) {
    for (const command of commandsIn(region)) found.add(command);
    if (depth > 0) {
      const { named, namespaces } = importsOf(file);
      const local = declarations(file);
      for (const match of region.matchAll(IDENTIFIER_PATTERN)) {
        const identifier = match[1] as string;
        if (identifier === name) continue;
        if (local.has(identifier)) {
          for (const command of commandsOfSymbol(file, identifier, depth - 1)) {
            found.add(command);
          }
          continue;
        }
        const imported = named.get(identifier);
        if (imported !== undefined) {
          for (const command of commandsOfSymbol(imported.file, imported.name, depth - 1)) {
            found.add(command);
          }
        }
      }
      for (const namespace of namespaces) {
        for (const command of commandsIn(SOURCE.get(namespace) ?? "")) found.add(command);
      }
    }
  }

  symbolCache.set(key, found);
  return found;
}

/** Commands a file can reach: its own literals plus the imports it actually references. */
function reachableCommands(file: string): ReadonlySet<string> {
  const text = SOURCE.get(file) ?? "";
  const found = new Set<string>(commandsIn(text));
  const { named, namespaces } = importsOf(file);
  const local = declarations(file);

  for (const match of text.matchAll(IDENTIFIER_PATTERN)) {
    const identifier = match[1] as string;
    if (local.has(identifier)) {
      for (const command of commandsOfSymbol(file, identifier, REACH_DEPTH)) found.add(command);
      continue;
    }
    const imported = named.get(identifier);
    if (imported !== undefined) {
      for (const command of commandsOfSymbol(imported.file, imported.name, REACH_DEPTH)) {
        found.add(command);
      }
    }
  }
  for (const namespace of namespaces) {
    for (const command of commandsIn(SOURCE.get(namespace) ?? "")) found.add(command);
  }
  return found;
}

// ---------------------------------------------------------------------------
// Affordance consumers
// ---------------------------------------------------------------------------

const AFFORDANCE_ROWS = Object.keys(AGENT_GATED_AFFORDANCES) as AgentGatedAffordance[];

const GATE_IMPORT = /from "@\/(?:hooks\/useAgentGate|lib\/remote\/agent-gate)"/;

/**
 * A file "resolves" an affordance when it imports the gate seam AND names the row.
 *
 * Naming rather than `useAgentGate("<row>")` specifically, because the row can also be
 * reached through a handler→affordance map (`TaskContextMenuItems`) feeding
 * `resolveAffordanceGate`. Both are the one gate; a row named anywhere else in a file
 * that never imports the gate is not a consumer.
 */
function consumersOf(affordance: AgentGatedAffordance): string[] {
  const named = new RegExp(`["']${affordance}["']`);
  return PRODUCTION_FILES.filter((file) => {
    if (file.endsWith(join("lib", "remote", "agent-gate.ts"))) return false;
    const text = SOURCE.get(file) ?? "";
    return GATE_IMPORT.test(text) && named.test(text);
  });
}

const CONSUMERS = new Map<AgentGatedAffordance, string[]>(
  AFFORDANCE_ROWS.map((affordance) => [affordance, consumersOf(affordance)])
);

const ALIASED_ROWS = new Set(
  AFFORDANCE_CONSUMPTION_ALIASES.map((alias) => alias.affordance)
);

const INDIRECT_PAIRS = new Set(
  GATE_CALLSITE_INDIRECTIONS.map((entry) => `${entry.file}::${entry.affordance}`)
);

// ---------------------------------------------------------------------------
// (a) Every affordance row has a production consumer
// ---------------------------------------------------------------------------

describe("every gated affordance row is consumed by a production surface", () => {
  const quarantined = quarantinedIds("dead-row");
  const deadRows = AFFORDANCE_ROWS.filter(
    (affordance) =>
      (CONSUMERS.get(affordance) ?? []).length === 0 && !ALIASED_ROWS.has(affordance)
  );

  it("has no dead row outside the quarantine", () => {
    const unexpected = deadRows.filter(
      (affordance) => !quarantined.has(`dead-row:${affordance}`)
    );
    expect(
      unexpected,
      "affordance rows with zero production consumers — either wire the surface or delete the row"
    ).toEqual([]);
  });

  it.each([...quarantined])("%s is still dead (ratchet)", (id) => {
    const affordance = id.slice("dead-row:".length) as AgentGatedAffordance;
    expect(
      deadRows,
      `${id} now HAS a consumer — delete its row from KNOWN_GATE_GAPS`
    ).toContain(affordance);
  });
});

// ---------------------------------------------------------------------------
// (b) A resolved affordance's op is a command the file can actually reach
// ---------------------------------------------------------------------------

describe("a resolved affordance names the op its file invokes", () => {
  const quarantined = quarantinedIds("op-mismatch");

  const pairs = AFFORDANCE_ROWS.flatMap((affordance) =>
    (CONSUMERS.get(affordance) ?? []).map((file) => ({
      affordance,
      file: rel(file),
      op: AGENT_GATED_AFFORDANCES[affordance],
      reaches: reachableCommands(file),
    }))
  );

  const mismatches = pairs.filter(
    (pair) =>
      !INDIRECT_PAIRS.has(`${pair.file}::${pair.affordance}`) &&
      !pair.reaches.has(pair.op)
  );

  it("covers at least the surfaces the wiring guard knows about", () => {
    // A resolution bug that silently found nothing would make every other assertion
    // in this file vacuous.
    expect(pairs.length).toBeGreaterThanOrEqual(20);
  });

  it("has no wrong-op gate outside the quarantine", () => {
    const unexpected = mismatches
      .map((pair) => `op-mismatch:${pair.file}::${pair.affordance}`)
      .filter((id) => !quarantined.has(id));
    expect(
      unexpected,
      "gate resolves an op the file never invokes — point the gate at the real op, or record the indirection"
    ).toEqual([]);
  });

  it.each([...quarantined])("%s is still mismatched (ratchet)", (id) => {
    const ids = mismatches.map((pair) => `op-mismatch:${pair.file}::${pair.affordance}`);
    expect(ids, `${id} now matches — delete its row from KNOWN_GATE_GAPS`).toContain(id);
  });
});

// ---------------------------------------------------------------------------
// Never invoke the raw twins
// ---------------------------------------------------------------------------

/**
 * The facade pins `resolve_permission_request` into `approve_permission_request` /
 * `deny_permission_request` by its `decision` argument, and denies `resolve_user_question`
 * in favour of `answer_user_question`. Invoking either raw name from production code is
 * unreachable at every scope remotely — and, for deny, takes a brake away from a
 * default-paired device.
 *
 * Only `invoke(` shapes count: naming the raw commands in a `LOCAL_ONLY_COMMANDS`-style
 * declaration or a comment is how the routing policy is expressed, not a defect.
 */
const RAW_TWINS = ["resolve_permission_request", "resolve_user_question"] as const;

const RAW_TWIN_PATTERN = new RegExp(
  `\\b(?:invoke|typedInvoke|typedInvokeWithTransform|typedInvokeVoid|networkInvoke)\\s*(?:<[^>()]*>)?\\(\\s*["'\`](${RAW_TWINS.join(
    "|"
  )})["'\`]`,
  "g"
);

describe("the facade-split raw commands are never invoked from production", () => {
  const quarantined = quarantinedIds("raw-twin");

  const sites = PRODUCTION_FILES.flatMap((file) =>
    [...(SOURCE.get(file) ?? "").matchAll(RAW_TWIN_PATTERN)].map(
      (match) => `raw-twin:${rel(file)}::${match[1] as string}`
    )
  );

  it("has no raw-twin invoke outside the quarantine", () => {
    const unexpected = sites.filter((id) => !quarantined.has(id));
    expect(
      unexpected,
      "route these through the pinned approve_/deny_permission_request and answer_user_question ops"
    ).toEqual([]);
  });

  it.each([...quarantined])("%s is still invoked (ratchet)", (id) => {
    expect(sites, `${id} is fixed — delete its row from KNOWN_GATE_GAPS`).toContain(id);
  });
});

// ---------------------------------------------------------------------------
// Quarantine hygiene
// ---------------------------------------------------------------------------

describe("the quarantine itself", () => {
  it("has unique ids", () => {
    const ids = KNOWN_GATE_GAPS.map((gap) => gap.id);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it("names an owning phase and a reason for every gap", () => {
    for (const gap of KNOWN_GATE_GAPS) {
      expect([1, 2, 4], `${gap.id} owner`).toContain(gap.owner);
      expect(gap.why.length, `${gap.id} needs a one-line reason`).toBeGreaterThan(20);
      expect(gap.id.startsWith(`${gap.kind}:`), `${gap.id} must be prefixed by its kind`).toBe(
        true
      );
    }
  });

  it("gives every indirection allowlist entry a reason", () => {
    for (const entry of [...GATE_CALLSITE_INDIRECTIONS, ...AFFORDANCE_CONSUMPTION_ALIASES]) {
      expect(entry.reason.length).toBeGreaterThan(20);
    }
  });
});
