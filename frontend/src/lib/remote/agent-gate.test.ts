/**
 * Derivation, staleness, and boundary tests for the `ui:agent` gate.
 *
 * These are the tests that make the gate trustworthy without a human re-reading 540
 * manifest rows: they re-derive the set from the manifest at test time and compare it
 * to the checked-in mirror, they prove the derivation refuses to produce a smaller
 * set when the input is degraded, and they cross-check both hand-maintained lists
 * (the affordance mapping and the inert exemptions) against the manifest.
 */

import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

import {
  AGENT_CONTROL_COMMANDS,
  AGENT_CONTROL_COMMAND_NAMES,
  AGENT_GATED_AFFORDANCES,
  INERT_AFFORDANCES,
  INERT_AFFORDANCE_COMMANDS,
  UI_AGENT_SCOPE,
  isAgentControlCommand,
  remoteErrorBannerProps,
  resolveAgentGate,
} from "./agent-gate";
import { AGENT_CONTROL_MANIFEST_SCHEMA_VERSION } from "./agent-control-commands.generated";
import { RemoteTransportError } from "./transport-errors";

const REPO_ROOT = resolve(__dirname, "../../../..");
const MANIFEST_PATH = resolve(REPO_ROOT, "docs/generated/remote-commands.json");

type Manifest = Record<string, unknown>;

function readManifest(): Manifest {
  return JSON.parse(readFileSync(MANIFEST_PATH, "utf8")) as Manifest;
}

async function derive(manifest: unknown): Promise<readonly string[]> {
  const module = await import(
    "../../../../scripts/lib/agent-control-derivation.mjs"
  );
  return (module.deriveGatedCommands as (m: unknown) => readonly string[])(manifest);
}

// ---------------------------------------------------------------------------
// The mirror actually mirrors
// ---------------------------------------------------------------------------

describe("gated command derivation", () => {
  it("matches the checked-in mirror (catches a stale generated file)", async () => {
    const derived = await derive(readManifest());
    expect([...AGENT_CONTROL_COMMANDS]).toEqual([...derived]);
  });

  it("records the manifest schema version it was generated from", () => {
    expect(AGENT_CONTROL_MANIFEST_SCHEMA_VERSION).toBe(
      readManifest().schemaVersion
    );
  });

  it("is a strict superset of the contract's floor-only union", () => {
    // The contract specified `agent_control_floor ∪ declared_memberships`. Against
    // this manifest that is 111 commands while 327 rows are classified
    // `class: "agentControl"`, so the contract's union under-gates steering by 261
    // commands. The derivation widens; this asserts it never NARROWS.
    const manifest = readManifest();
    const floorUnion = new Set([
      ...(manifest.agent_control_floor as string[]),
      ...(manifest.declared_memberships as ReadonlyArray<{ command: string }>).map(
        (row) => row.command
      ),
    ]);
    for (const command of floorUnion) {
      expect(AGENT_CONTROL_COMMAND_NAMES.has(command), command).toBe(true);
    }
    expect(AGENT_CONTROL_COMMANDS.length).toBeGreaterThan(floorUnion.size);
  });

  it("gates every command the ledger classifies as agentControl", () => {
    const ledger = readManifest().ledger as ReadonlyArray<{
      command: string;
      class: string;
    }>;
    const missing = ledger
      .filter((row) => row.class === "agentControl")
      .map((row) => row.command)
      .filter((command) => !AGENT_CONTROL_COMMAND_NAMES.has(command));
    expect(missing).toEqual([]);
  });

  it("never gates an operate-class or read-class command", () => {
    // The viewer-with-brakes floor: widening the set must not swallow the brakes or
    // the read surface a paired device is always allowed.
    const ledger = readManifest().ledger as ReadonlyArray<{
      command: string;
      class: string;
    }>;
    const swallowed = ledger
      .filter((row) => row.class === "operate" || row.class === "read")
      .map((row) => row.command)
      .filter((command) => AGENT_CONTROL_COMMAND_NAMES.has(command));
    expect(swallowed).toEqual([]);
  });

  it("pins the floor anchors from the contract", () => {
    for (const anchor of [
      "move_task",
      "inject_task",
      "resume_automation",
      "approve_permission_request",
      "resolve_user_question",
      "answer_user_question",
      "unblock_task",
    ]) {
      expect(isAgentControlCommand(anchor)).toBe(true);
    }
  });
});

// ---------------------------------------------------------------------------
// Fail closed — a degraded manifest must never yield a SMALLER gated set
// ---------------------------------------------------------------------------

describe("derivation fails closed", () => {
  it("throws when the manifest file is missing", async () => {
    // The loader, pointed at a path that does not exist: the mirror generator must
    // abort rather than emit an empty (= nothing gated) set.
    const { execFileSync } = await import("node:child_process");
    expect(() =>
      execFileSync(
        "node",
        [
          resolve(REPO_ROOT, "scripts/check-agent-control-command-mirror.mjs"),
          resolve(REPO_ROOT, "does-not-exist"),
        ],
        { stdio: "pipe" }
      )
    ).toThrow(/manifest is missing/);
  });

  it("throws rather than returning an empty set for a non-object manifest", async () => {
    await expect(derive(null)).rejects.toThrow(/not an object/);
  });

  it("throws when any coverage flag is not complete", async () => {
    for (const flag of ["detectorA", "detectorB", "agentConsumedContent"]) {
      const manifest = readManifest();
      manifest.coverage = { ...(manifest.coverage as object), [flag]: "pending" };
      await expect(derive(manifest)).rejects.toThrow(
        new RegExp(`coverage\\.${flag}`)
      );
    }
  });

  it("throws when the coverage block is absent entirely", async () => {
    const manifest = readManifest();
    delete manifest.coverage;
    await expect(derive(manifest)).rejects.toThrow(/no coverage block/);
  });

  it("throws when the derived union would be empty", async () => {
    const manifest = readManifest();
    manifest.agent_control_floor = [];
    manifest.declared_memberships = [];
    manifest.ledger = [];
    await expect(derive(manifest)).rejects.toThrow(/empty/);
  });

  it("throws when declared_memberships rows lose their command field", async () => {
    const manifest = readManifest();
    manifest.declared_memberships = [{ reason: "steering-question" }];
    await expect(derive(manifest)).rejects.toThrow(/no string command/);
  });
});

// ---------------------------------------------------------------------------
// Staleness guard on the hand-maintained mapping table
// ---------------------------------------------------------------------------

describe("affordance mapping stays aligned with the manifest", () => {
  it("names only commands the manifest classifies as agent-control", () => {
    const unmapped: string[] = [];
    for (const [affordance, commands] of Object.entries(AGENT_GATED_AFFORDANCES)) {
      for (const command of commands) {
        if (!AGENT_CONTROL_COMMAND_NAMES.has(command)) {
          unmapped.push(`${affordance} -> ${command}`);
        }
      }
    }
    // A host-side rename lands here as a red test instead of an ungated button.
    expect(unmapped).toEqual([]);
  });

  it("covers every affordance with at least one command", () => {
    for (const [affordance, commands] of Object.entries(AGENT_GATED_AFFORDANCES)) {
      expect(commands.length, affordance).toBeGreaterThan(0);
    }
  });
});

// ---------------------------------------------------------------------------
// A6 / R6-M1 — the inert list is closed, and no note surface is in it
// ---------------------------------------------------------------------------

describe("inert exemption list", () => {
  it("is exactly the closed six-surface set", () => {
    expect([...INERT_AFFORDANCES]).toEqual([
      "permissionDeny",
      "stop",
      "pause",
      "block",
      "taskEditCategoryPriority",
      "backlogCreate",
    ]);
  });

  it("declares an argument constraint for every gated command it exempts", () => {
    // The honest version of "the inert list must not intersect the gated set". Two
    // A6 surfaces genuinely share a command with a steering action, so the rule is
    // not "no intersection" but "no UNEXPLAINED intersection": an inert row that
    // exempts a gated command must say which argument restriction makes it safe.
    const unexplained: string[] = [];
    for (const [affordance, row] of Object.entries(INERT_AFFORDANCE_COMMANDS)) {
      for (const command of row.commands) {
        if (AGENT_CONTROL_COMMAND_NAMES.has(command) && row.argumentConstraint === null) {
          unexplained.push(`${affordance} -> ${command}`);
        }
      }
    }
    expect(unexplained).toEqual([]);
  });

  it("keeps the manifest's own authority-reducing commands unconstrained-safe", () => {
    // Cross-check against the manifest's `authority_reducing_exemptions` table: every
    // command it lists must be absent from the gated set, so the brakes never depend
    // on our argument-constraint reasoning.
    const exemptions = readManifest().authority_reducing_exemptions as ReadonlyArray<{
      kind?: string;
      command?: string;
    }>;
    const ledgerCommands = new Set(
      (readManifest().ledger as ReadonlyArray<{ command: string }>).map(
        (row) => row.command
      )
    );
    const commandExemptions = exemptions
      .filter((row) => row.kind === "command" && typeof row.command === "string")
      .map((row) => row.command as string)
      // `deny_permission_request` is a declared BRANCH, not a registered command.
      .filter((command) => ledgerCommands.has(command));

    expect(commandExemptions.length).toBeGreaterThan(0);
    for (const command of commandExemptions) {
      expect(AGENT_CONTROL_COMMAND_NAMES.has(command), command).toBe(false);
    }
  });

  it("contains no note-writing surface (R6-M1)", () => {
    for (const affordance of INERT_AFFORDANCES) {
      expect(affordance.toLowerCase()).not.toContain("note");
    }
    for (const row of Object.values(INERT_AFFORDANCE_COMMANDS)) {
      for (const command of row.commands) {
        expect(command).not.toContain("note");
      }
    }
  });

  it("keeps every note-writing command out of the inert set", () => {
    // `add_task_note` does not exist in this manifest revision, so R6-M1's literal
    // anchor is asserted conditionally and its INTENT is asserted over whatever note
    // commands the manifest actually carries: a note surface is agent-control or
    // outright denied, never inert.
    const manifest = readManifest();
    const ledger = manifest.ledger as ReadonlyArray<{
      command: string;
      class: string;
    }>;
    const inertCommands = new Set(
      Object.values(INERT_AFFORDANCE_COMMANDS).flatMap((row) => [...row.commands])
    );

    const noteWriters = ledger.filter(
      (row) =>
        row.command.includes("note") &&
        !/^(get|list)_/.test(row.command) &&
        !row.command.includes("release_notes")
    );
    expect(noteWriters.length).toBeGreaterThan(0);

    for (const row of noteWriters) {
      expect(inertCommands.has(row.command), row.command).toBe(false);
      expect(["agentControl", "denied"], row.command).toContain(row.class);
    }

    if (AGENT_CONTROL_COMMAND_NAMES.has("add_task_note")) {
      expect(isAgentControlCommand("add_task_note")).toBe(true);
    }
  });
});

// ---------------------------------------------------------------------------
// Gate resolution — scope truth, including the fail-closed cases
// ---------------------------------------------------------------------------

describe("resolveAgentGate", () => {
  it("never gates a local environment", () => {
    expect(resolveAgentGate(false, null)).toEqual({ gated: false, reason: null });
    expect(resolveAgentGate(false, [])).toEqual({ gated: false, reason: null });
  });

  it("enables when the live scopes include ui:agent", () => {
    const gate = resolveAgentGate(true, ["ui:read", "ui:operate", UI_AGENT_SCOPE]);
    expect(gate.gated).toBe(false);
  });

  it("gates a default-paired remote environment", () => {
    const gate = resolveAgentGate(true, ["ui:read", "ui:operate"]);
    expect(gate.gated).toBe(true);
    expect(gate.reason).toBe(
      "Agent control is off for this device — enable it on the host."
    );
  });

  it("gates when introspection has never confirmed a set", () => {
    // `null` is "unknown", not "empty" — optimism here would authorize steering on
    // a connection whose scopes were never proven.
    expect(resolveAgentGate(true, null).gated).toBe(true);
    expect(resolveAgentGate(true, undefined).gated).toBe(true);
  });

  it("gates on an empty confirmed set", () => {
    expect(resolveAgentGate(true, []).gated).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// A7 — the error banner mapper
// ---------------------------------------------------------------------------

describe("remoteErrorBannerProps", () => {
  const build = (code: string) =>
    new RemoteTransportError({
      code: code as never,
      message: "denied",
      environmentId: "env-remote",
    });

  it("maps REMOTE_FORBIDDEN to the gate explanation", () => {
    const props = remoteErrorBannerProps(build("REMOTE_FORBIDDEN"));
    expect(props?.tone).toBe("error");
    expect(props?.body).toBe(
      "Agent control is off for this device — enable it on the host."
    );
  });

  it("maps REMOTE_COMMAND_UNAVAILABLE to a host-capability message", () => {
    const props = remoteErrorBannerProps(build("REMOTE_COMMAND_UNAVAILABLE"));
    expect(props?.title).toBe("Unavailable on this host");
  });

  it("passes REMOTE_UNAUTHORIZED through untouched (2.7 owns it)", () => {
    expect(remoteErrorBannerProps(build("REMOTE_UNAUTHORIZED"))).toBeNull();
  });

  it("claims no other transport code, including the unknown-outcome pair", () => {
    for (const code of [
      "REMOTE_UNREACHABLE",
      "REMOTE_VERSION_MISMATCH",
      "REMOTE_TIMEOUT_UNKNOWN",
      "REMOTE_REQUEST_IN_PROGRESS",
      "REMOTE_REQUEST_ID_REUSED",
      "REMOTE_INVALID_ARGUMENTS",
      "REMOTE_INTERNAL_ERROR",
    ]) {
      expect(remoteErrorBannerProps(build(code)), code).toBeNull();
    }
  });

  it("ignores non-transport errors", () => {
    expect(remoteErrorBannerProps(new Error("boom"))).toBeNull();
    expect(remoteErrorBannerProps(null)).toBeNull();
  });
});
