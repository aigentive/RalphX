/**
 * Remote affordance availability (PR 2.6-b), against manifest schemaVersion 2.
 *
 * When a host revokes `ui:agent`, a paired remote environment can still watch and STOP
 * things — deny, stop, pause, block, edit
 * category/priority, create a Backlog task. Everything that steers an agent forward
 * needs the `ui:agent` scope that pairing grants by default and the host can revoke.
 *
 * ## Three states, not two
 *
 * The manifest's `facade_ops` table (schema v2) is the set of operations the host
 * actually exposes remotely. That makes two failure modes distinct, and conflating
 * them produces actively misleading UI:
 *
 * - `unavailable` — the command is not in `facade_ops`. No scope grant reaches it;
 *   telling the user to "enable it on the host" points at a switch that will not
 *   help. Derived from ABSENCE, never from a name list: three commands are
 *   unregistered today as detector-c process-launch rejections, and that set moves
 *   with the host, so hardcoding them would rot silently.
 * - `gated` — the op exists and is `class: agentControl`, but the live confirmed
 *   scopes lack `ui:agent`. This one IS fixable on the host, and says so.
 * - `enabled` — reachable and authorized (or the environment is local).
 *
 * ## Argument-level cases
 *
 * Some ops carry both a steering and an inert action. `update_task` is
 * `class: operate` but `conditional_capabilities` escalates `title`/`description` to
 * agent control; `resolve_permission_request` is split by the facade into pinned
 * `approve_permission_request` / `deny_permission_request` ops. So affordances name
 * the FACADE OP they front, not the underlying Tauri command, and the field-level
 * case is resolved by `resolveFieldGate`. Gating either command wholesale would take
 * the brakes away from every device whose agent control was revoked.
 *
 * Unknown scope state gates closed: `null` effective scopes means the supervisor has
 * never confirmed a set (Fixed Decision 2), not an empty grant to be optimistic about.
 */

import {
  REMOTE_CONDITIONAL_CAPABILITIES,
  REMOTE_FACADE_OPS,
  REMOTE_MANIFEST_SCHEMA_VERSION,
  type RemoteFacadeOp,
} from "@/lib/remote/remote-capabilities.generated";
import {
  isRemoteTransportError,
  type RemoteTransportError,
} from "@/lib/remote/transport-errors";

export {
  REMOTE_FACADE_OPS,
  REMOTE_CONDITIONAL_CAPABILITIES,
  REMOTE_MANIFEST_SCHEMA_VERSION,
};

/** The scope that authorizes agent steering (`api/remote-host.ts` schema). */
export const UI_AGENT_SCOPE = "ui:agent";

/** Exact copy for a scope-gated control. Fixed by contract — do not reword. */
export const AGENT_CONTROL_DISABLED_HINT =
  "Agent control is off for this device — enable it on the host.";

/**
 * Copy for an op the host does not expose remotely at all. Deliberately does NOT
 * suggest a host setting: there is no switch that turns this on.
 */
export const REMOTE_UNAVAILABLE_HINT =
  "This action runs only on the host — it is not available remotely.";

// ---------------------------------------------------------------------------
// Affordance ↦ facade op mapping (hand-maintained; guarded by tests)
// ---------------------------------------------------------------------------

/**
 * Every UI surface this PR gates, named after the affordance rather than the
 * component, mapped to the facade op it fronts.
 *
 * These are FACADE op names, which is why `permissionApprove` maps to
 * `approve_permission_request` rather than `resolve_permission_request` — the facade
 * splits that command by its pinned `decision` argument.
 *
 * An affordance whose op is absent from `REMOTE_FACADE_OPS` resolves to
 * `unavailable`. That is not an error and no test asserts membership: absence is the
 * signal, and several affordances are legitimately unavailable today.
 */
export const AGENT_GATED_AFFORDANCES = {
  ticketingProvidersRead: "list_ticketing_providers",
  ticketingStatusCatalogRead: "list_ticketing_status_catalog",
  ticketingAssociationsRead: "get_ticket_associations",
  ticketingConversationRead: "get_conversation_ticket",
  ticketingRefresh: "refresh_tickets",
  // Refresh launches provider discovery on the host and is intentionally absent remotely.
  // The catalog read itself routes through the registered spawn-free snapshot twin.
  mcpCatalogRefresh: "refresh_mcp_catalog",
  // Repository capability is re-inspected by this command. It is intentionally absent
  // remotely until that host-side operation has a reviewed facade twin; absence is the signal.
  repositorySettingsPrToggle: "update_github_pr_enabled",
  // Both send affordances resolve through the spawn-free facade command, NOT
  // `send_agent_message`. That command reaches a process-launch sink and stays
  // unregistered by ruling, so pointing the gate at it would render chat send
  // permanently `unavailable` on every paired device.
  agentComposerSend: "send_remote_chat_message",
  chatSend: "send_remote_chat_message",
  // The IDLE half of remote send (WP1). `send_remote_chat_message` above only reaches a
  // conversation a run is already serving; continuing an idle one goes through the
  // continuation-intent command instead. Both are `agentControl`, so the two rows never
  // disagree on scope — the split exists so a host that predates WP1 resolves the
  // continuation affordance `unavailable` (absence, never hardcoded) while live sends keep
  // working, rather than presenting one control whose behaviour silently halved.
  chatContinueIdle: "request_remote_agent_conversation_message",
  // Starting a conversation routes through the spawn-free facade command, NOT
  // `start_agent_conversation` — that command fires the process-spawn detectors and
  // stays unregistered by ruling, so pointing the gate at it would render Start
  // permanently `unavailable` on every paired host. Until the host registers the
  // spawn-free command it is absent from `REMOTE_FACADE_OPS` and Start resolves
  // `unavailable` (older-host row) — derived from absence, never hardcoded.
  startConversation: "request_remote_agent_conversation_start",
  // Stopping routes through the spawn-free facade command, NOT `stop_agent` — that command
  // reaches `Command::new(resolve_pkill_cli_path())` and stays unregistered by the absolute
  // process floor, so pointing the gate at it would render Stop permanently `unavailable`.
  // The intent is `class: operate`, so `resolveAffordanceGate` returns `enabled` for the
  // DEFAULT pairing: brakes must not need `ui:agent`. Against an older host that predates the
  // registration the op is simply absent from `REMOTE_FACADE_OPS` and Stop renders the
  // unavailable hint instead of an enabled button that answers `REMOTE_COMMAND_UNAVAILABLE` —
  // derived from absence, never hardcoded.
  agentStop: "request_remote_agent_stop",
  // Switching a conversation's mode routes through the spawn-free mode-switch intent
  // (WP5a), NOT `switch_agent_conversation_mode` — that command prepares the workspace
  // (`ensure_git_worktree`, a git spawn) and stays unregistered by the absolute process
  // floor. The intent is `agentControl`, so the mode picker renders gated below `ui:agent`
  // and unavailable against an older host that predates the registration — derived from
  // absence, never hardcoded.
  conversationModeSwitch: "request_remote_agent_conversation_mode_switch",
  planApprove: "request_remote_plan_approval",
  planArtifactEdit: "request_remote_plan_artifact_edit",
  ideationAcceptFinalize: "request_remote_ideation_finalize_decision",
  ideationRejectFinalize: "request_remote_ideation_finalize_decision",
  planDirectImplementation: "activate_agent_plan_direct_implementation",
  planTaskPipeline: "start_agent_task_pipeline",
  conversationFork: "request_remote_conversation_fork",
  conversationArchive: "request_remote_conversation_archive",
  conversationUnarchive: "restore_agent_conversation",
  conversationMute: "set_remote_agent_conversation_muted",
  personaSwitch: "switch_remote_agent_conversation_persona",
  attachmentUpload: "upload_chat_attachment",
  // Metadata uses the path-free read twin. Older hosts omit the row, so the transcript keeps
  // the honest unknown state from manifest absence instead of trying the local path-bearing op.
  attachmentMetadataRead: "list_remote_message_attachments",
  permissionApprove: "approve_permission_request",
  // Answering the MCP `ask_user_question` gate uses the registered, spawn-free remote twin.
  // It is `agentControl`, so a remote needs `ui:agent`; older hosts without the manifest row
  // resolve unavailable from absence. `answer_user_question` remains background history, not
  // a twin: it requires `taskId`, performs a Blocked→Ready task transition, and never signals
  // the MCP long-poll keyed by `requestId`. The twin instead resolves that pending request.
  // Accepted plan-mode proposals are refused host-side before commit because they prepare a
  // workspace; the claim is released fail-closed so the still-pending banner remains retryable.
  // Ordinary late answers still commit with `deliveredToWaitingAgent: false`; the client then
  // compensates through `formatLateQuestionAnswer`, whose chat send uses the registered
  // `send_remote_chat_message` remote command.
  questionAnswer: "resolve_remote_user_question",
  taskMove: "move_task",
  // Task-level stop is registered as agentControl, so remotes without ui:agent must not invoke it.
  taskStop: "stop_task",
  // Resume/restart route through the registered spawn-free intent twins. Older hosts omit
  // these rows, so availability still derives from manifest absence.
  taskRestart: "request_remote_task_restart",
  taskApprove: "approve_task_for_review",
  taskResume: "request_remote_task_resume",
  groupResume: "request_remote_group_resume",
  recoveryPromptResolve: "request_remote_recovery_prompt_resolution",
  taskUnblock: "unblock_task",
  // Merge recovery commands are not registered on the remote facade.
  mergeRetry: "retry_merge",
  mergeResolveConflict: "resolve_merge_conflict",
  // Reviewing feedback remains local-only; execution resume uses its registered intent twin.
  taskRequestChangesFromReviewing: "request_task_changes_from_reviewing",
  executionResume: "request_remote_execution_resume",
  executionPlanPause: "pause_execution_plan",
  executionPlanResume: "resume_execution_plan",
  executionPlanStop: "stop_execution_plan",
  applyProposals: "apply_proposals_to_kanban",
  taskEditContent: "update_task",
  stepCreate: "create_task_step",
  stepUpdate: "update_task_step",
  stepSkip: "skip_step",
  proposalEdit: "update_task_proposal",
  // Attaching a folder to a conversation. Unavailable remotely for TWO independent reasons
  // and derived, as always, from absence: the host keeps `add_conversation_folder_reference`
  // off the facade (the stored path becomes an MCP filesystem root for every later spawn, with
  // no project-root allowlist), and the picker that produces the path is a native dialog on
  // THIS Mac, so the path would not exist on the host anyway. Its `list`/`remove` siblings ARE
  // registered, so an existing reference still renders and can still be detached.
  folderReferenceAdd: "add_conversation_folder_reference",
  folderReferenceRemove: "remove_conversation_folder_reference",
  // Publishing and closing a PR both reach host processes and remain unregistered.
  // Paired clients still render the registered workspace/review snapshot reads; only
  // these two controls resolve unavailable from manifest absence.
  workspacePublish: "publish_agent_conversation_workspace",
  workspaceClosePr: "close_agent_workspace_pr",
  // Wave B3c wires the B3a/B3b spawn-free queue twins. Edit shares the cancel row because its
  // first, deciding step is cancel; its already-registered send half remains unchanged.
  queuedMessageDelete: "cancel_remote_queued_agent_message",
  queuedMessageEdit: "cancel_remote_queued_agent_message",
  queuedMessageSendNow: "request_remote_queued_message_send",
  automationResume: "resume_automation",
  // Automation lifecycle controls are host-classified `agentControl`, including
  // authority-reducing pause/stop, matching the task pause/stop precedent below.
  automationPause: "pause_automation",
  automationStop: "stop_automation",
  automationRunNow: "request_remote_automation_run",
  automationRestart: "restart_automation",
  automationCancelRun: "cancel_automation_run",
  automationResumeRun: "resume_automation_run",
  automationRetryPlanJudge: "retry_automation_plan_judge",
  automationSkipJudge: "skip_automation_judge",
  automationSettingsEdit: "update_automation_settings",
  automationFinalize: "finalize_automation",
  automationRetryJudge: "request_remote_automation_run",
  // Automation deletion remains absent from the remote facade at the D2 floor, so
  // these controls resolve `unavailable` at every remote scope.
  automationDelete: "delete_automation",
  automationDeleteRun: "delete_automation_run",
  automationSetupEdit: "update_automation_config",
  automationCreate: "request_remote_automation_draft",
} as const satisfies Record<string, string>;

export type AgentGatedAffordance = keyof typeof AGENT_GATED_AFFORDANCES;

/**
 * The closed inert exemption list (A6) — surfaces that stay fully operable under a
 * remote environment without `ui:agent` because they only ever REDUCE authority or create
 * un-armed work. Closed means closed: adding a row is a boundary change.
 *
 * The per-task brakes (`stop`, `pause`, `block`) were exempt until the host escalated
 * `stop_task` / `pause_task` / `pause_tasks_in_group` / `block_task` to `AgentControl`.
 * An affordance the host refuses without the agent-control grant is not inert, whatever
 * direction it moves authority in, so they left this list rather than presenting an
 * enabled control that answers `REMOTE_FORBIDDEN`.
 */
export const INERT_AFFORDANCES = [
  "permissionDeny",
  "taskEditCategoryPriority",
  "backlogCreate",
] as const;

export type InertAffordance = (typeof INERT_AFFORDANCES)[number];

/**
 * Facade ops backing the inert affordances.
 *
 * Every one of these must resolve to `class: read | operate` — never `agentControl`.
 * The manifest now makes that checkable, which is why the cross-check test asserts
 * the CLASS rather than trusting a hand-written justification.
 */
export const INERT_AFFORDANCE_OPS = {
  permissionDeny: ["deny_permission_request"],
  taskEditCategoryPriority: ["update_task"],
  backlogCreate: ["create_task"],
} as const satisfies Record<InertAffordance, readonly string[]>;

// ---------------------------------------------------------------------------
// Resolution
// ---------------------------------------------------------------------------

/**
 * `read_only` (2.7-a) is a fourth, TRANSIENT status: the op exists and this device is
 * authorized, but the active remote environment has no confirmed connection to write
 * through. It differs from `gated` (fix it on the host) and `unavailable` (nothing
 * fixes it) in that it clears by itself when the supervisor reconnects.
 */
export type AgentGateStatus =
  | "enabled"
  | "gated"
  | "unavailable"
  | "read_only";

export interface AgentGateState {
  readonly status: AgentGateStatus;
  /** `true` when the affordance must be disabled — gated OR unavailable. */
  readonly gated: boolean;
  /** Tooltip/aria copy when disabled, `null` when enabled. */
  readonly reason: string | null;
}

const ENABLED: AgentGateState = { status: "enabled", gated: false, reason: null };
const GATED: AgentGateState = {
  status: "gated",
  gated: true,
  reason: AGENT_CONTROL_DISABLED_HINT,
};
const UNAVAILABLE: AgentGateState = {
  status: "unavailable",
  gated: true,
  reason: REMOTE_UNAVAILABLE_HINT,
};

/**
 * Folds degraded-connection read-only mode into an already-resolved gate.
 *
 * Precedence is deliberate: `unavailable` WINS. An op the host does not expose
 * remotely stays "not available remotely" even mid-reconnect, because reconnecting
 * will not make it appear and saying otherwise sends the user to wait for nothing.
 * Everything else yields to read-only, including `gated` — while there is no
 * connection, the scope question is moot.
 */
export function withReadOnly(
  gate: AgentGateState,
  writable: boolean,
  reason: string | null
): AgentGateState {
  if (writable || gate.status === "unavailable") {
    return gate;
  }
  return {
    status: "read_only",
    gated: true,
    // Presentation-neutral: this fallback is reached only when the caller supplied no
    // reason, and asserting "reconnecting" over a syncing environment would contradict
    // the calm chip. The specific copy comes from `useEnvironmentWritable`.
    reason: reason ?? "This environment isn't connected — changes can't be made right now.",
  };
}

function hasAgentScope(scopes: readonly string[] | null | undefined): boolean {
  return scopes !== null && scopes !== undefined && scopes.includes(UI_AGENT_SCOPE);
}

/** The facade op backing a command name, or `null` when it is not exposed remotely. */
export function facadeOpFor(command: string): RemoteFacadeOp | null {
  return REMOTE_FACADE_OPS[command] ?? null;
}

/**
 * Resolves one affordance for the active environment.
 *
 * @param affordance the A3 row being rendered
 * @param isRemoteEnvironment whether the ACTIVE environment is remote
 * @param effectiveScopes the LIVE confirmed scopes — `null`/`undefined` when
 *   introspection has never succeeded, which gates closed. The pair-time
 *   `remote.scopes` snapshot must never be passed here.
 */
export function resolveAffordanceGate(
  affordance: AgentGatedAffordance,
  isRemoteEnvironment: boolean,
  effectiveScopes: readonly string[] | null | undefined
): AgentGateState {
  if (!isRemoteEnvironment) return ENABLED;

  const command = AGENT_GATED_AFFORDANCES[affordance];
  const op = facadeOpFor(command);
  if (op === null) return UNAVAILABLE;

  // `update_task` is `class: operate` overall; the content fields escalate it. A
  // caller naming `taskEditContent` is asking about the escalated half.
  if (affordance === "taskEditContent") {
    return hasAgentScope(effectiveScopes) ? ENABLED : GATED;
  }

  if (op.opClass !== "agentControl") return ENABLED;
  return hasAgentScope(effectiveScopes) ? ENABLED : GATED;
}

/**
 * Resolves the scope-only question: "may this device steer at all?"
 *
 * For surfaces that have no single backing op, or where the caller only needs the
 * broad answer. It cannot distinguish `unavailable`, so prefer
 * `resolveAffordanceGate` wherever an affordance name exists.
 */
export function resolveAgentGate(
  isRemoteEnvironment: boolean,
  effectiveScopes: readonly string[] | null | undefined
): AgentGateState {
  if (!isRemoteEnvironment) return ENABLED;
  return hasAgentScope(effectiveScopes) ? ENABLED : GATED;
}

/**
 * Whether a specific FIELD of an argument-sensitive op needs `ui:agent`.
 *
 * Drives the task-edit form: title/description lock while category/priority stay
 * live, matching what the host's `update_task_authz` will actually enforce.
 */
export function resolveFieldGate(
  command: string,
  field: string,
  isRemoteEnvironment: boolean,
  effectiveScopes: readonly string[] | null | undefined
): AgentGateState {
  if (!isRemoteEnvironment) return ENABLED;
  if (facadeOpFor(command) === null) return UNAVAILABLE;

  const conditional = REMOTE_CONDITIONAL_CAPABILITIES[command];
  if (conditional === undefined || !conditional.fields.includes(field)) {
    return ENABLED;
  }
  return hasAgentScope(effectiveScopes) ? ENABLED : GATED;
}

/** Whether an op is reachable remotely at all. */
export function isRemotelyAvailable(command: string): boolean {
  return facadeOpFor(command) !== null;
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
        body: REMOTE_UNAVAILABLE_HINT,
      };
    default:
      return null;
  }
}
