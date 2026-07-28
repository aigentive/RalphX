/**
 * Pure derivation of the `ui:agent`-gated command set from
 * `docs/generated/remote-commands.json` (PR 2.6-b).
 *
 * Kept side-effect-free and separate from the CLI so tests can import it without the
 * checker's file reads and `process.exit` running on import.
 *
 * ## Why this is a UNION of three sources, not the contract's two
 *
 * The 2.6 contract specified `agent_control_floor ∪ declared_memberships`. Against
 * the manifest as it exists at this HEAD that yields 111 commands — while the ledger
 * classifies 327 commands as `class: "agentControl"`, of which 261 are NOT in that
 * union. `agent_control_floor` turns out to be a different axis entirely (45 of its
 * members are `denied`/`elevated` rather than `agentControl`), so the contract's
 * union under-gates agent steering by 261 commands.
 *
 * Under-gating is the dangerous direction — it leaves a steering affordance live for
 * a device the host never granted `ui:agent`. So the derived set is the union of all
 * three sources. It is a strict superset of the contract's set, contains zero
 * `operate`-class or `read`-class commands (asserted in `agent-gate.test.ts`, so the
 * viewer-with-brakes floor is provably untouched), and any `denied`/`elevated` member
 * inherited from the floor is already blocked by a stricter rule anyway.
 */

const REQUIRED_COVERAGE = ["detectorA", "detectorB", "agentConsumedContent"];

/**
 * @param {unknown} manifest parsed `remote-commands.json`
 * @returns {readonly string[]} sorted, de-duplicated gated command names
 * @throws when the manifest is degraded in any way that would SHRINK the set
 */
export function deriveGatedCommands(manifest) {
  if (manifest === null || typeof manifest !== "object") {
    throw new Error("remote-commands manifest is not an object");
  }

  const coverage = manifest.coverage;
  if (coverage === null || typeof coverage !== "object") {
    throw new Error("remote-commands manifest has no coverage block");
  }
  for (const flag of REQUIRED_COVERAGE) {
    if (coverage[flag] !== "complete") {
      throw new Error(
        `remote-commands manifest coverage.${flag} is ${JSON.stringify(
          coverage[flag]
        )}; the derived gate set would be structurally under-complete`
      );
    }
  }

  const ledger = manifest.ledger;
  if (!Array.isArray(ledger)) {
    throw new Error("remote-commands manifest ledger is not an array");
  }
  const agentControlClass = ledger.map((row) => {
    if (row === null || typeof row !== "object" || typeof row.command !== "string") {
      throw new Error("remote-commands manifest ledger row has no string command");
    }
    return row.class === "agentControl" ? row.command : null;
  });

  const floor = manifest.agent_control_floor;
  if (!Array.isArray(floor) || !floor.every((name) => typeof name === "string")) {
    throw new Error("remote-commands manifest agent_control_floor is not string[]");
  }

  const memberships = manifest.declared_memberships;
  if (!Array.isArray(memberships)) {
    throw new Error("remote-commands manifest declared_memberships is not an array");
  }
  const membershipCommands = memberships.map((row) => {
    if (row === null || typeof row !== "object" || typeof row.command !== "string") {
      throw new Error(
        "remote-commands manifest declared_memberships row has no string command"
      );
    }
    return row.command;
  });

  const union = [
    ...new Set(
      [
        ...agentControlClass.filter((name) => name !== null),
        ...floor,
        ...membershipCommands,
      ].filter((name) => typeof name === "string")
    ),
  ].sort();

  if (union.length === 0) {
    throw new Error("derived agent-control command set is empty");
  }
  return union;
}
