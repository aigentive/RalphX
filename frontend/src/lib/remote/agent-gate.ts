/**
 * The `ui:agent` capability gate (PR 2.6-b).
 *
 * Under the viewer-with-brakes boundary (Fixed Decision 12) a default-paired remote
 * environment can watch and it can STOP things — deny, stop, pause, block, edit
 * category/priority, create a Backlog task. Everything that steers an agent forward
 * needs the `ui:agent` scope the host grants explicitly.
 *
 * Three rules shape this module:
 *
 * 1. **The gated SET is never hand-maintained.** It comes from
 *    `docs/generated/remote-commands.json` via the checked-in mirror
 *    (`agent-control-commands.generated.ts`, Decision 1). A hand list would drift
 *    from the host's own classification the first time a command was renamed, and
 *    the failure mode of that drift is silently ungating a steering command.
 * 2. **What IS hand-maintained is the affordance↔command mapping** — which UI
 *    surface fronts which command — and the closed inert exemption list, because
 *    those are claims about this codebase's UI, not about the host's command table.
 *    Both are guarded by tests that cross-check them against the manifest.
 * 3. **Unknown scope state gates closed.** `null` effective scopes means the
 *    supervisor has never confirmed a set (Fixed Decision 2); it is not an empty
 *    grant to be optimistic about.
 *
 * Note the asymmetry with 2.6-a: host-impossible affordances HIDE, permission-gated
 * affordances DISABLE. A hidden control says "this can never work here"; a disabled
 * one with this module's hint says "someone can turn this on", which is true.
 */

import {
  AGENT_CONTROL_COMMAND_NAMES,
  AGENT_CONTROL_COMMANDS,
} from "@/lib/remote/agent-control-commands.generated";
import {
  isRemoteTransportError,
  type RemoteTransportError,
} from "@/lib/remote/transport-errors";

export { AGENT_CONTROL_COMMANDS, AGENT_CONTROL_COMMAND_NAMES };

/** The scope that authorizes agent steering (`api/remote-host.ts` schema). */
export const UI_AGENT_SCOPE = "ui:agent";

/** Exact copy for a gated control's tooltip. Fixed by contract — do not reword. */
export const AGENT_CONTROL_DISABLED_HINT =
  "Agent control is off for this device — enable it on the host.";

// ---------------------------------------------------------------------------
// Affordance ↦ command mapping (hand-maintained; guarded by tests)
// ---------------------------------------------------------------------------

/**
 * Every UI surface this PR gates, named after the affordance rather than the
 * component, plus the command(s) it ultimately dispatches.
 *
 * The commands are here so the mapping is FALSIFIABLE: a test asserts each one is
 * present in the manifest union, which turns a host-side rename into a red test
 * instead of a silently ungated button.
 */
export const AGENT_GATED_AFFORDANCES = {
  agentComposerSend: ["send_agent_message"],
  chatSend: ["send_agent_message"],
  startConversation: ["start_agent_conversation", "create_agent_conversation"],
  permissionApprove: ["approve_permission_request"],
  questionAnswer: ["resolve_user_question", "answer_user_question"],
  taskMove: ["move_task"],
  taskApprove: ["approve_task_for_review"],
  taskResume: ["resume_task", "restart_task", "resume_tasks_in_group"],
  taskUnblock: ["unblock_task"],
  applyProposals: ["apply_proposals_to_kanban"],
  taskEditContent: ["update_task"],
  stepMutations: ["create_task_step", "update_task_step", "skip_step"],
  proposalEdit: ["update_task_proposal"],
  automationControl: [
    "trigger_automation_run_now",
    "restart_automation",
    "resume_automation",
  ],
} as const satisfies Record<string, readonly string[]>;

export type AgentGatedAffordance = keyof typeof AGENT_GATED_AFFORDANCES;

/**
 * The closed inert exemption list (A6) — surfaces that stay fully operable under a
 * default-paired remote environment because they only ever REDUCE authority or create
 * un-armed work.
 *
 * Closed means closed: adding a row is a boundary change (Fixed Decision 12), and the
 * derivation test asserts the list is exactly this set.
 */
export const INERT_AFFORDANCES = [
  "permissionDeny",
  "stop",
  "pause",
  "block",
  "taskEditCategoryPriority",
  "backlogCreate",
] as const;

export type InertAffordance = (typeof INERT_AFFORDANCES)[number];

export interface InertAffordanceRow {
  /** The command(s) the affordance dispatches. */
  readonly commands: readonly string[];
  /**
   * The ARGUMENT restriction that makes this affordance authority-reducing, when the
   * command it dispatches is not authority-reducing on its own. `null` means the
   * command is unconditionally safe and the manifest agrees.
   *
   * This field exists because the boundary is not always command-shaped. Two A6
   * surfaces share a command with a steering action — `resolve_permission_request`
   * carries both allow and deny, `create_task` can target any column — so exempting
   * the COMMAND would hand the remote device the steering half too. Recording the
   * constraint here makes the narrower exemption reviewable instead of implicit, and
   * the test below refuses any inert row that is in the gated set WITHOUT one.
   */
  readonly argumentConstraint: string | null;
}

export const INERT_AFFORDANCE_COMMANDS = {
  permissionDeny: {
    commands: ["resolve_permission_request"],
    argumentConstraint:
      'decision must be "deny"; the allow branch is gated (declared_memberships: approve_permission_request)',
  },
  stop: {
    commands: ["stop_task", "stop_execution", "stop_agent"],
    // `stop_task` is manifest `class: operate`; the two process-level stops are
    // classified agentControl conservatively but can only ever halt work.
    argumentConstraint: "halt-only; no target state other than stopped",
  },
  pause: {
    commands: ["pause_task", "pause_tasks_in_group", "pause_execution"],
    argumentConstraint: "halt-only; no target state other than paused",
  },
  block: {
    commands: ["block_task"],
    argumentConstraint: null,
  },
  taskEditCategoryPriority: {
    // Shares `update_task` with title/description editing, which IS gated: the
    // category/priority fields do not feed agent-consumed content.
    commands: ["update_task"],
    argumentConstraint:
      "diff may contain only category and/or priority; title and description are gated",
  },
  backlogCreate: {
    commands: ["create_task"],
    argumentConstraint:
      "target column must be draft or backlog; creating into an armed column is gated",
  },
} as const satisfies Record<InertAffordance, InertAffordanceRow>;

// ---------------------------------------------------------------------------
// Gate evaluation
// ---------------------------------------------------------------------------

export interface AgentGateState {
  /** `true` when the affordance must be disabled and explained. */
  readonly gated: boolean;
  /** Tooltip/aria copy when gated, `null` otherwise. */
  readonly reason: string | null;
}

const ENABLED: AgentGateState = { gated: false, reason: null };
const GATED: AgentGateState = {
  gated: true,
  reason: AGENT_CONTROL_DISABLED_HINT,
};

/**
 * Resolves the gate for the active environment.
 *
 * @param isRemoteEnvironment whether the ACTIVE environment is remote
 * @param effectiveScopes the LIVE confirmed scopes for it — `null`/`undefined` when
 *   introspection has never succeeded, which gates closed. The pair-time
 *   `remote.scopes` snapshot must never be passed here.
 */
export function resolveAgentGate(
  isRemoteEnvironment: boolean,
  effectiveScopes: readonly string[] | null | undefined
): AgentGateState {
  if (!isRemoteEnvironment) return ENABLED;
  if (effectiveScopes === null || effectiveScopes === undefined) return GATED;
  return effectiveScopes.includes(UI_AGENT_SCOPE) ? ENABLED : GATED;
}

/** Whether a raw command name requires `ui:agent`, per the generated manifest set. */
export function isAgentControlCommand(command: string): boolean {
  return AGENT_CONTROL_COMMAND_NAMES.has(command);
}

// ---------------------------------------------------------------------------
// Error banner (A7)
// ---------------------------------------------------------------------------

export interface RemoteErrorBannerProps {
  readonly tone: "error";
  readonly title: string;
  readonly body: string;
}

/**
 * Standard inline presentation for the two authorization/availability codes a gated
 * surface can still hit when a scope narrows mid-flight.
 *
 * Deliberately narrow. `REMOTE_UNAUTHORIZED` belongs to 2.7 (blocked/credential
 * presentation) and passes through untouched, and every other code — including the
 * unknown-outcome pair, whose correct handling is refetch-not-resend — keeps its
 * existing call-site treatment. Returning `null` rather than a generic banner is what
 * stops this mapper from quietly swallowing errors it has nothing useful to say about.
 */
export function remoteErrorBannerProps(
  error: unknown
): RemoteErrorBannerProps | null {
  if (!isRemoteTransportError(error)) return null;
  const transportError: RemoteTransportError = error;

  switch (transportError.code) {
    case "REMOTE_FORBIDDEN":
      return {
        tone: "error",
        title: "Not allowed for this device",
        body: AGENT_CONTROL_DISABLED_HINT,
      };
    case "REMOTE_COMMAND_UNAVAILABLE":
      return {
        tone: "error",
        title: "Unavailable on this host",
        body: "The host does not offer this action remotely.",
      };
    default:
      return null;
  }
}
