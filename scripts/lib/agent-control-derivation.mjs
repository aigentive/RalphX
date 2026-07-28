/**
 * Pure derivation of the remote capability model from
 * `docs/generated/remote-commands.json` (PR 2.6-b, manifest schemaVersion 2).
 *
 * Kept side-effect-free and separate from the CLI so tests can import it without the
 * checker's file reads and `process.exit` running on import.
 *
 * ## What schema v2 changed, and why the client got simpler
 *
 * v1 forced the client to infer the remote surface from classification tables
 * (`agent_control_floor`, the ledger's `class` column) that describe how RISKY a
 * command is, not whether it is reachable at all. v2 adds `facade_ops`: the exact
 * set of operations the host facade actually exposes remotely, each with its scope
 * class, its argument pins, and whether it is argument-sensitive. That is the real
 * authority, so the client keys on it.
 *
 * The important consequence is that "the host does not offer this remotely" and
 * "your device lacks the scope" are now DIFFERENT answers, and the UI must not
 * conflate them. A command absent from `facade_ops` is unavailable no matter what
 * the host grants; granting `ui:agent` would not make it appear. Telling a user to
 * go enable agent control on the host for such an affordance sends them to a switch
 * that cannot help. This is derived from absence, never from a name list — three
 * commands are deliberately unregistered today (detector-c process-launch
 * rejections) and that set will move.
 *
 * `conditional_capabilities` carries the argument-level cases: `update_task` needs
 * `ui:agent` only for `title`/`description`; category/priority edits stay inside
 * `ui:operate`. Gating the command as a whole would take the inert-metadata edit
 * away from every default-paired device.
 *
 * ## Fail-closed rules
 *
 * Throws — never returns a smaller/emptier model — on: a missing or non-object
 * manifest, any `coverage.*` flag that is not `"complete"`, an unknown
 * `schemaVersion`, a malformed `facade_ops`, or an empty op set. An UNKNOWN
 * schemaVersion (> the supported max) throws rather than being treated as the
 * newest known version: a future revision could add an op class or a pin shape this
 * consumer would silently mis-read as permissive.
 */

const REQUIRED_COVERAGE = ["detectorA", "detectorB", "agentConsumedContent"];

/** The manifest revisions this consumer understands. */
export const SUPPORTED_SCHEMA_VERSIONS = [2];

/** Scope classes a facade op can carry. */
export const OP_CLASSES = ["read", "operate", "agentControl"];

function assertCoverage(manifest) {
  const coverage = manifest.coverage;
  if (coverage === null || typeof coverage !== "object") {
    throw new Error("remote-commands manifest has no coverage block");
  }
  for (const flag of REQUIRED_COVERAGE) {
    if (coverage[flag] !== "complete") {
      throw new Error(
        `remote-commands manifest coverage.${flag} is ${JSON.stringify(
          coverage[flag]
        )}; the derived capability model would be structurally under-complete`
      );
    }
  }
}

function assertSchemaVersion(manifest) {
  const version = manifest.schemaVersion;
  if (typeof version !== "number") {
    throw new Error("remote-commands manifest has no numeric schemaVersion");
  }
  if (!SUPPORTED_SCHEMA_VERSIONS.includes(version)) {
    throw new Error(
      `remote-commands manifest schemaVersion ${version} is not supported ` +
        `(this consumer understands ${SUPPORTED_SCHEMA_VERSIONS.join(", ")}); ` +
        "refusing to guess at an unknown capability model"
    );
  }
}

/**
 * @param {unknown} manifest parsed `remote-commands.json`
 * @returns {{ schemaVersion: number, ops: Array<object>, conditionals: Array<object> }}
 */
export function deriveRemoteCapabilityModel(manifest) {
  if (manifest === null || typeof manifest !== "object") {
    throw new Error("remote-commands manifest is not an object");
  }
  assertSchemaVersion(manifest);
  assertCoverage(manifest);

  const facadeOps = manifest.facade_ops;
  if (!Array.isArray(facadeOps)) {
    throw new Error("remote-commands manifest facade_ops is not an array");
  }

  const ops = facadeOps.map((row) => {
    if (row === null || typeof row !== "object" || typeof row.command !== "string") {
      throw new Error("remote-commands manifest facade_ops row has no string command");
    }
    if (!OP_CLASSES.includes(row.class)) {
      throw new Error(
        `remote-commands manifest facade op ${row.command} has unknown class ` +
          `${JSON.stringify(row.class)}`
      );
    }
    const pins = Array.isArray(row.pins) ? row.pins : [];
    return {
      command: row.command,
      opClass: row.class,
      argumentSensitive: row.argumentSensitive === true,
      capabilities: Array.isArray(row.capabilities) ? [...row.capabilities] : [],
      pins: pins.map((pin) => ({
        param: String(pin.param ?? ""),
        field: String(pin.field ?? ""),
        value: pin.value,
      })),
    };
  });

  if (ops.length === 0) {
    throw new Error("derived remote facade op set is empty");
  }
  if (!ops.some((op) => op.opClass === "agentControl")) {
    // A model with no agent-control ops would gate nothing at all.
    throw new Error("derived remote facade op set has no agentControl ops");
  }

  const rawConditionals = manifest.conditional_capabilities;
  if (!Array.isArray(rawConditionals)) {
    throw new Error(
      "remote-commands manifest conditional_capabilities is not an array"
    );
  }
  const opCommands = new Set(ops.map((op) => op.command));
  const conditionals = rawConditionals.map((row) => {
    if (row === null || typeof row !== "object" || typeof row.command !== "string") {
      throw new Error(
        "remote-commands manifest conditional_capabilities row has no string command"
      );
    }
    if (!opCommands.has(row.command)) {
      throw new Error(
        `conditional capability names ${row.command}, which is not a facade op`
      );
    }
    return {
      command: row.command,
      capability: String(row.capability ?? ""),
      condition: String(row.condition ?? ""),
      // "conditional: title,description — discharged by ..." → ["title","description"]
      fields: parseConditionFields(row.condition),
    };
  });

  ops.sort((a, b) => a.command.localeCompare(b.command));
  conditionals.sort((a, b) => a.command.localeCompare(b.command));
  return { schemaVersion: manifest.schemaVersion, ops, conditionals };
}

/**
 * Pulls the field list out of a condition string.
 *
 * The condition is prose with a machine-readable head (`conditional: a,b — …`).
 * Parsing is strict about that head and throws otherwise rather than returning an
 * empty field list, which downstream would read as "no fields are restricted" — the
 * permissive direction.
 */
export function parseConditionFields(condition) {
  const text = String(condition ?? "");
  const match = /^conditional:\s*([^—\-]+)/.exec(text);
  if (match === null) {
    throw new Error(
      `conditional capability condition ${JSON.stringify(text)} has no ` +
        '"conditional: <fields>" head; refusing to infer an empty field set'
    );
  }
  const fields = match[1]
    .split(",")
    .map((field) => field.trim())
    .filter((field) => field.length > 0);
  if (fields.length === 0) {
    throw new Error(
      `conditional capability condition ${JSON.stringify(text)} lists no fields`
    );
  }
  return fields;
}
