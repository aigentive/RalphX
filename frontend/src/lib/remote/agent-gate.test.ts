/**
 * Derivation, staleness, and boundary tests for the remote capability model
 * (manifest schemaVersion 2).
 *
 * These are the tests that make the gate trustworthy without a human re-reading the
 * manifest: they re-derive the model from it at test time and compare to the
 * checked-in mirror, they prove the derivation refuses to produce a smaller or more
 * permissive model when the input is degraded, and they cross-check both
 * hand-maintained lists (the affordance mapping and the inert exemptions) against the
 * manifest's own classification.
 */

import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

import {
  AGENT_CONTROL_DISABLED_HINT,
  AGENT_GATED_AFFORDANCES,
  INERT_AFFORDANCES,
  INERT_AFFORDANCE_OPS,
  REMOTE_CONDITIONAL_CAPABILITIES,
  REMOTE_FACADE_OPS,
  REMOTE_MANIFEST_SCHEMA_VERSION,
  REMOTE_UNAVAILABLE_HINT,
  UI_AGENT_SCOPE,
  isRemotelyAvailable,
  remoteErrorBannerProps,
  resolveAffordanceGate,
  resolveAgentGate,
  resolveFieldGate,
} from "./agent-gate";
import { RemoteTransportError } from "./transport-errors";

const REPO_ROOT = resolve(__dirname, "../../../..");
const MANIFEST_PATH = resolve(REPO_ROOT, "docs/generated/remote-commands.json");

type Manifest = Record<string, unknown>;

function readManifest(): Manifest {
  return JSON.parse(readFileSync(MANIFEST_PATH, "utf8")) as Manifest;
}

async function derivation() {
  return await import("../../../../scripts/lib/agent-control-derivation.mjs");
}

async function derive(manifest: unknown) {
  const module = await derivation();
  return (
    module.deriveRemoteCapabilityModel as (m: unknown) => {
      schemaVersion: number;
      ops: Array<{ command: string; opClass: string }>;
      conditionals: Array<{ command: string; fields: string[] }>;
    }
  )(manifest);
}

const GRANTED = ["ui:read", "ui:operate", UI_AGENT_SCOPE];
const WITHOUT_UI_AGENT = ["ui:read", "ui:operate"];

// ---------------------------------------------------------------------------
// The mirror actually mirrors
// ---------------------------------------------------------------------------

describe("capability model derivation", () => {
  it("matches the checked-in mirror (catches a stale generated file)", async () => {
    const model = await derive(readManifest());
    expect(Object.keys(REMOTE_FACADE_OPS).sort()).toEqual(
      model.ops.map((op) => op.command).sort()
    );
    for (const op of model.ops) {
      expect(REMOTE_FACADE_OPS[op.command]?.opClass, op.command).toBe(op.opClass);
    }
  });

  it("records the manifest schema version it was generated from", () => {
    expect(REMOTE_MANIFEST_SCHEMA_VERSION).toBe(readManifest().schemaVersion);
  });

  it("mirrors the conditional capability for update_task", () => {
    expect(REMOTE_CONDITIONAL_CAPABILITIES["update_task"]?.fields).toEqual([
      "title",
      "description",
    ]);
  });

  it("classifies the facade ops the boundary depends on", () => {
    // Steering ops the host DOES expose.
    for (const command of [
      "move_task",
      "unblock_task",
      "approve_task_for_review",
      "approve_permission_request",
      "answer_user_question",
      "resume_automation",
      // The per-task brakes were escalated to agent control on the host; the mirror is the
      // authority for that, so pin it here rather than in the inert list.
      "pause_task",
      "block_task",
      "stop_task",
      "pause_tasks_in_group",
    ]) {
      expect(REMOTE_FACADE_OPS[command]?.opClass, command).toBe("agentControl");
    }
    // Inert work available without ui:agent.
    for (const command of ["deny_permission_request", "create_task", "update_task"]) {
      expect(REMOTE_FACADE_OPS[command]?.opClass, command).toBe("operate");
    }
  });

  it("treats an unregistered command as unavailable, not scope-forbidden", () => {
    // Derived from ABSENCE. These three are detector-c process-launch rejections
    // today; the assertion is about the mechanism, not the names.
    for (const command of [
      "resume_task",
      "apply_proposals_to_kanban",
      "set_agent_conversation_workspace_auto_publish",
    ]) {
      expect(isRemotelyAvailable(command), command).toBe(false);
    }
  });
});

// ---------------------------------------------------------------------------
// Fail closed — a degraded manifest must never yield a more permissive model
// ---------------------------------------------------------------------------

describe("derivation fails closed", () => {
  it("throws when the manifest file is missing", async () => {
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

  it("throws for a non-object manifest", async () => {
    await expect(derive(null)).rejects.toThrow(/not an object/);
  });

  it("throws on an UNKNOWN schemaVersion rather than assuming the newest", async () => {
    // The important one: a v3 manifest could add an op class or pin shape this
    // consumer would mis-read as permissive, so it must refuse rather than guess.
    const manifest = readManifest();
    manifest.schemaVersion = 3;
    await expect(derive(manifest)).rejects.toThrow(/schemaVersion 3 is not supported/);
  });

  it("throws on a missing schemaVersion", async () => {
    const manifest = readManifest();
    delete manifest.schemaVersion;
    await expect(derive(manifest)).rejects.toThrow(/no numeric schemaVersion/);
  });

  it("throws when any coverage flag is not complete", async () => {
    for (const flag of ["detectorA", "detectorB", "agentConsumedContent"]) {
      const manifest = readManifest();
      manifest.coverage = { ...(manifest.coverage as object), [flag]: "pending" };
      await expect(derive(manifest)).rejects.toThrow(new RegExp(`coverage\\.${flag}`));
    }
  });

  it("throws when facade_ops is empty or has no agent-control op", async () => {
    const empty = readManifest();
    empty.facade_ops = [];
    empty.conditional_capabilities = [];
    await expect(derive(empty)).rejects.toThrow(/empty/);

    const noSteering = readManifest();
    noSteering.facade_ops = [
      { command: "list_tasks", class: "read", pins: [], capabilities: [] },
    ];
    noSteering.conditional_capabilities = [];
    await expect(derive(noSteering)).rejects.toThrow(/no agentControl ops/);
  });

  it("throws on an unknown op class", async () => {
    const manifest = readManifest();
    manifest.facade_ops = [
      { command: "mystery_op", class: "superuser", pins: [], capabilities: [] },
    ];
    manifest.conditional_capabilities = [];
    await expect(derive(manifest)).rejects.toThrow(/unknown class/);
  });

  it("throws when a conditional names a command that is not a facade op", async () => {
    const manifest = readManifest();
    manifest.conditional_capabilities = [
      { command: "not_an_op", capability: "x", condition: "conditional: title" },
    ];
    await expect(derive(manifest)).rejects.toThrow(/not a facade op/);
  });

  it("refuses to infer an empty field set from an unparseable condition", async () => {
    // An empty field list would read downstream as "no fields are restricted".
    const { parseConditionFields } = await derivation();
    const parse = parseConditionFields as (condition: unknown) => string[];
    expect(() => parse("who knows")).toThrow(/no "conditional: <fields>" head/);
    expect(() => parse("conditional:   — discharged")).toThrow(/lists no fields/);
    expect(parse("conditional: title,description — discharged by x")).toEqual([
      "title",
      "description",
    ]);
  });
});

// ---------------------------------------------------------------------------
// Staleness guard on the hand-maintained mapping table
// ---------------------------------------------------------------------------

describe("affordance mapping", () => {
  it("maps every affordance to a non-empty command name", () => {
    for (const [affordance, command] of Object.entries(AGENT_GATED_AFFORDANCES)) {
      expect(command.length, affordance).toBeGreaterThan(0);
    }
  });

  it("routes plan and finalize affordances through their registered intent twins", () => {
    expect(AGENT_GATED_AFFORDANCES.planApprove).toBe("request_remote_plan_approval");
    expect(AGENT_GATED_AFFORDANCES.planArtifactEdit).toBe(
      "request_remote_plan_artifact_edit",
    );
    expect(AGENT_GATED_AFFORDANCES.ideationAcceptFinalize).toBe(
      "request_remote_ideation_finalize_decision",
    );
    expect(AGENT_GATED_AFFORDANCES.ideationRejectFinalize).toBe(
      "request_remote_ideation_finalize_decision",
    );
    for (const affordance of [
      "planApprove",
      "planArtifactEdit",
      "ideationAcceptFinalize",
      "ideationRejectFinalize",
    ] as const) {
      expect(resolveAffordanceGate(affordance, true, GRANTED).status).toBe("enabled");
    }
  });

  it("keeps post-approval process continuations unavailable remotely", () => {
    expect(resolveAffordanceGate("planDirectImplementation", true, GRANTED).status).toBe(
      "unavailable",
    );
    expect(resolveAffordanceGate("planTaskPipeline", true, GRANTED).status).toBe(
      "unavailable",
    );
  });

  it("names facade ops, not the underlying command, for pinned splits", () => {
    // `resolve_permission_request` is split by the facade into two pinned ops; an
    // affordance naming the raw command would gate deny along with approve.
    expect(AGENT_GATED_AFFORDANCES.permissionApprove).toBe(
      "approve_permission_request"
    );
    expect(INERT_AFFORDANCE_OPS.permissionDeny).toEqual([
      "deny_permission_request",
    ]);
    expect(REMOTE_FACADE_OPS["approve_permission_request"]?.pins).toContainEqual(
      expect.objectContaining({ field: "decision", value: "allow" })
    );
    expect(REMOTE_FACADE_OPS["deny_permission_request"]?.pins).toContainEqual(
      expect.objectContaining({ field: "decision", value: "deny" })
    );
  });

  /**
   * Affordances whose backing op is a BRAKE, and therefore deliberately reachable from the
   * device without ui:agent. This is a deliberate carve-out, not a hole: `request_remote_agent_stop` is
   * `class: operate` (authority-REDUCING, with a recorded `AUTHORITY_REDUCING_EXEMPTIONS` row),
   * so gating it would take the brake away from exactly the devices with agent control revoked
   * boundary exists to give it to. Adding a row here is a boundary change — the op's class is
   * asserted below so a reclassified op cannot ride in on this list.
   */
  // Whole-scheduler pause/stop joined 2026-08-04: both ops are registered `operate`
  // (authority-reducing — they halt work, never start it), so their affordances must stay
  // usable on a default pairing exactly like the per-conversation stop brake.
  const DEFAULT_PAIRING_BRAKES = ["agentStop", "executionPause", "executionStop"] as const;

  it("resolves every affordance to a defined state without ui:agent", () => {
    for (const affordance of Object.keys(
      AGENT_GATED_AFFORDANCES
    ) as Array<keyof typeof AGENT_GATED_AFFORDANCES>) {
      const state = resolveAffordanceGate(affordance, true, WITHOUT_UI_AGENT);
      const expected = (DEFAULT_PAIRING_BRAKES as readonly string[]).includes(
        affordance
      )
        ? ["enabled", "unavailable"]
        : ["gated", "unavailable"];
      expect(expected, affordance).toContain(state.status);
    }
  });

  it("every brake available without ui:agent is backed by a read/operate op", () => {
    for (const affordance of DEFAULT_PAIRING_BRAKES) {
      const op = REMOTE_FACADE_OPS[AGENT_GATED_AFFORDANCES[affordance]];
      // Absent is fine (older host → `unavailable`); present-and-escalated is not.
      if (op === undefined) continue;
      expect(["read", "operate"], affordance).toContain(op.opClass);
    }
  });
});

// ---------------------------------------------------------------------------
// A6 / R6-M1 — the inert list is closed and manifest-backed
// ---------------------------------------------------------------------------

describe("inert exemption list", () => {
  it("is exactly the closed three-surface set", () => {
    // `stop` / `pause` / `block` left the list when the host escalated the per-task brakes
    // to agent control — see the `INERT_AFFORDANCES` doc comment.
    expect([...INERT_AFFORDANCES]).toEqual([
      "permissionDeny",
      "taskEditCategoryPriority",
      "backlogCreate",
    ]);
  });

  it("names only ops the manifest serves under read or operate", () => {
    // Manifest-backed now: no hand-written justification is trusted.
    const escalated: string[] = [];
    for (const [affordance, commands] of Object.entries(INERT_AFFORDANCE_OPS)) {
      for (const command of commands) {
        const op = REMOTE_FACADE_OPS[command];
        expect(op, `${affordance} -> ${command}`).toBeDefined();
        if (op !== undefined && op.opClass === "agentControl") {
          escalated.push(`${affordance} -> ${command}`);
        }
      }
    }
    expect(escalated).toEqual([]);
  });

  it("contains no note-writing surface (R6-M1)", () => {
    for (const affordance of INERT_AFFORDANCES) {
      expect(affordance.toLowerCase()).not.toContain("note");
    }
    for (const commands of Object.values(INERT_AFFORDANCE_OPS)) {
      for (const command of commands) {
        expect(command).not.toContain("note");
      }
    }
  });

  it("keeps every note-writing command out of the remote operate surface", () => {
    // `add_task_note` does not exist in this manifest, so R6-M1's literal anchor is
    // asserted conditionally and its INTENT over whatever note commands exist: a note
    // surface is never served below ui:agent.
    const ledger = readManifest().ledger as ReadonlyArray<{
      command: string;
      class: string;
    }>;
    const noteWriters = ledger.filter(
      (row) =>
        row.command.includes("note") &&
        !/^(get|list)_/.test(row.command) &&
        !row.command.includes("release_notes")
    );
    expect(noteWriters.length).toBeGreaterThan(0);

    for (const row of noteWriters) {
      const op = REMOTE_FACADE_OPS[row.command];
      if (op !== undefined) {
        expect(op.opClass, row.command).toBe("agentControl");
      }
    }

    const addTaskNote = REMOTE_FACADE_OPS["add_task_note"];
    if (addTaskNote !== undefined) {
      expect(addTaskNote.opClass).toBe("agentControl");
    }
  });
});

// ---------------------------------------------------------------------------
// Three-state resolution
// ---------------------------------------------------------------------------

describe("resolveAffordanceGate", () => {
  it("never gates a local environment, even for an unavailable op", () => {
    expect(resolveAffordanceGate("taskResume", false, null).status).toBe("enabled");
    expect(resolveAffordanceGate("chatSend", false, null).status).toBe("enabled");
  });

  it("reports an unregistered op as unavailable regardless of scopes", () => {
    // The load-bearing distinction: granting ui:agent does NOT make it appear, so
    // the copy must not send the user to a host switch.
    for (const scopes of [null, WITHOUT_UI_AGENT, GRANTED]) {
      const state = resolveAffordanceGate("taskResume", true, scopes);
      expect(state.status).toBe("unavailable");
      expect(state.reason).toBe(REMOTE_UNAVAILABLE_HINT);
      expect(state.reason).not.toBe(AGENT_CONTROL_DISABLED_HINT);
    }
  });

  it("gates a registered agent-control op when ui:agent is absent", () => {
    const state = resolveAffordanceGate("taskMove", true, WITHOUT_UI_AGENT);
    expect(state.status).toBe("gated");
    expect(state.reason).toBe(AGENT_CONTROL_DISABLED_HINT);
  });

  it("enables a registered agent-control op when ui:agent is granted", () => {
    expect(resolveAffordanceGate("taskMove", true, GRANTED).status).toBe("enabled");
    expect(resolveAffordanceGate("taskApprove", true, GRANTED).status).toBe(
      "enabled"
    );
  });

  it("gates when introspection has never confirmed a set", () => {
    expect(resolveAffordanceGate("taskMove", true, null).status).toBe("gated");
    expect(resolveAffordanceGate("taskMove", true, undefined).status).toBe("gated");
    expect(resolveAffordanceGate("taskMove", true, []).status).toBe("gated");
  });

  it("splits the folder-reference pair: add is unavailable, remove is merely gated", () => {
    // WP4 (a) registered the reducing half and left the widening half off the facade. The two
    // states are different answers to the user: `remove` says "enable it on the host",
    // `add` says nothing will.
    expect(REMOTE_FACADE_OPS["add_conversation_folder_reference"]).toBeUndefined();
    expect(REMOTE_FACADE_OPS["remove_conversation_folder_reference"]?.opClass).toBe(
      "agentControl"
    );
    // The read half is what keeps existing references rendering remotely.
    expect(REMOTE_FACADE_OPS["list_conversation_folder_references"]?.opClass).toBe("read");

    for (const scopes of [null, WITHOUT_UI_AGENT, GRANTED]) {
      const add = resolveAffordanceGate("folderReferenceAdd", true, scopes);
      expect(add.status).toBe("unavailable");
      expect(add.reason).toBe(REMOTE_UNAVAILABLE_HINT);
    }
    expect(resolveAffordanceGate("folderReferenceRemove", true, WITHOUT_UI_AGENT).status).toBe(
      "gated"
    );
    expect(resolveAffordanceGate("folderReferenceRemove", true, GRANTED).status).toBe(
      "enabled"
    );
    expect(resolveAffordanceGate("folderReferenceAdd", false, null).status).toBe("enabled");
  });

  it("exposes the task-step write ops the transport-shape deferral used to hide", () => {
    // These were unregistered on a finding that was never true, so `stepSkip` rendered
    // `unavailable` on every paired device even with ui:agent granted.
    for (const command of [
      "start_step",
      "complete_step",
      "skip_step",
      "fail_step",
      "reorder_task_steps",
    ]) {
      expect(REMOTE_FACADE_OPS[command]?.opClass).toBe("agentControl");
    }
    expect(resolveAffordanceGate("stepSkip", true, WITHOUT_UI_AGENT).status).toBe("gated");
    expect(resolveAffordanceGate("stepSkip", true, GRANTED).status).toBe("enabled");
  });

  it("gates the content half of update_task while the op itself is operate", () => {
    expect(REMOTE_FACADE_OPS["update_task"]?.opClass).toBe("operate");
    expect(resolveAffordanceGate("taskEditContent", true, WITHOUT_UI_AGENT).status).toBe(
      "gated"
    );
    expect(resolveAffordanceGate("taskEditContent", true, GRANTED).status).toBe(
      "enabled"
    );
  });
});

describe("resolveFieldGate", () => {
  it("locks the conditional fields and leaves the inert ones editable", () => {
    for (const field of ["title", "description"]) {
      expect(
        resolveFieldGate("update_task", field, true, WITHOUT_UI_AGENT).status,
        field
      ).toBe("gated");
    }
    for (const field of ["category", "priority"]) {
      expect(
        resolveFieldGate("update_task", field, true, WITHOUT_UI_AGENT).status,
        field
      ).toBe("enabled");
    }
  });

  it("unlocks the conditional fields once ui:agent is granted", () => {
    expect(resolveFieldGate("update_task", "title", true, GRANTED).status).toBe(
      "enabled"
    );
  });

  it("is inert on local", () => {
    expect(resolveFieldGate("update_task", "title", false, null).status).toBe(
      "enabled"
    );
  });

  it("reports an unavailable op as unavailable, not field-gated", () => {
    expect(
      resolveFieldGate("resume_task", "title", true, GRANTED).status
    ).toBe("unavailable");
  });
});

describe("resolveAgentGate (scope-only fallback)", () => {
  it("answers the broad question without an affordance", () => {
    expect(resolveAgentGate(false, null).status).toBe("enabled");
    expect(resolveAgentGate(true, WITHOUT_UI_AGENT).status).toBe("gated");
    expect(resolveAgentGate(true, GRANTED).status).toBe("enabled");
    expect(resolveAgentGate(true, null).status).toBe("gated");
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

  it("maps REMOTE_FORBIDDEN to the scope explanation", () => {
    const props = remoteErrorBannerProps(build("REMOTE_FORBIDDEN"));
    expect(props?.tone).toBe("error");
    expect(props?.body).toBe(AGENT_CONTROL_DISABLED_HINT);
  });

  it("maps REMOTE_COMMAND_UNAVAILABLE to the availability explanation", () => {
    const props = remoteErrorBannerProps(build("REMOTE_COMMAND_UNAVAILABLE"));
    expect(props?.title).toBe("Unavailable on this host");
    expect(props?.body).toBe(REMOTE_UNAVAILABLE_HINT);
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
