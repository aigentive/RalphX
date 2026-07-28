// GENERATED — do not edit; run node scripts/check-agent-control-command-mirror.mjs --update

/** `schemaVersion` of the manifest this mirror was generated from. */
export const REMOTE_MANIFEST_SCHEMA_VERSION = 2;

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
  /** Argument values the facade pins, e.g. `decision: "deny"`. */
  readonly pins: readonly RemoteFacadePin[];
}

/**
 * Every operation the host facade exposes remotely. A command ABSENT from this map is
 * not reachable remotely at all — no scope grant changes that.
 */
export const REMOTE_FACADE_OPS: Readonly<Record<string, RemoteFacadeOp>> = {
  "add_artifact_relation": {
    opClass: "agentControl",
    argumentSensitive: false,
    capabilities: ["mutatesAgentConsumedContent"],
    pins: [],
  },
  "answer_user_question": {
    opClass: "agentControl",
    argumentSensitive: false,
    capabilities: ["agentControl"],
    pins: [],
  },
  "approve_permission_request": {
    opClass: "agentControl",
    argumentSensitive: false,
    capabilities: ["agentControl"],
    pins: [{"param":"args","field":"decision","value":"allow"}],
  },
  "approve_task_for_review": {
    opClass: "agentControl",
    argumentSensitive: false,
    capabilities: ["mutatesAgentConsumedContent"],
    pins: [],
  },
  "block_task": {
    opClass: "operate",
    argumentSensitive: false,
    capabilities: [],
    pins: [],
  },
  "cancel_tasks_in_group": {
    opClass: "operate",
    argumentSensitive: false,
    capabilities: [],
    pins: [],
  },
  "create_artifact": {
    opClass: "agentControl",
    argumentSensitive: false,
    capabilities: ["mutatesAgentConsumedContent"],
    pins: [],
  },
  "create_task": {
    opClass: "operate",
    argumentSensitive: false,
    capabilities: [],
    pins: [],
  },
  "create_task_step": {
    opClass: "agentControl",
    argumentSensitive: false,
    capabilities: ["mutatesAgentConsumedContent"],
    pins: [],
  },
  "deny_permission_request": {
    opClass: "operate",
    argumentSensitive: false,
    capabilities: [],
    pins: [{"param":"args","field":"decision","value":"deny"}],
  },
  "finalize_automation": {
    opClass: "agentControl",
    argumentSensitive: false,
    capabilities: ["seedsSpawnTriggeringState"],
    pins: [],
  },
  "get_active_project": {
    opClass: "read",
    argumentSensitive: false,
    capabilities: [],
    pins: [],
  },
  "get_archived_count": {
    opClass: "read",
    argumentSensitive: false,
    capabilities: [],
    pins: [],
  },
  "get_execution_settings": {
    opClass: "read",
    argumentSensitive: false,
    capabilities: [],
    pins: [],
  },
  "get_global_execution_settings": {
    opClass: "read",
    argumentSensitive: false,
    capabilities: [],
    pins: [],
  },
  "get_session_task_history_availability": {
    opClass: "read",
    argumentSensitive: false,
    capabilities: [],
    pins: [],
  },
  "get_step_progress": {
    opClass: "read",
    argumentSensitive: false,
    capabilities: [],
    pins: [],
  },
  "get_task": {
    opClass: "read",
    argumentSensitive: false,
    capabilities: [],
    pins: [],
  },
  "get_task_agent_workspace": {
    opClass: "read",
    argumentSensitive: false,
    capabilities: [],
    pins: [],
  },
  "get_task_dependency_graph": {
    opClass: "read",
    argumentSensitive: false,
    capabilities: [],
    pins: [],
  },
  "get_task_state_transitions": {
    opClass: "read",
    argumentSensitive: false,
    capabilities: [],
    pins: [],
  },
  "get_task_steps": {
    opClass: "read",
    argumentSensitive: false,
    capabilities: [],
    pins: [],
  },
  "get_task_timeline_events": {
    opClass: "read",
    argumentSensitive: false,
    capabilities: [],
    pins: [],
  },
  "get_tasks_awaiting_review": {
    opClass: "read",
    argumentSensitive: false,
    capabilities: [],
    pins: [],
  },
  "get_valid_transitions": {
    opClass: "read",
    argumentSensitive: false,
    capabilities: [],
    pins: [],
  },
  "health_check": {
    opClass: "read",
    argumentSensitive: false,
    capabilities: [],
    pins: [],
  },
  "inject_task": {
    opClass: "agentControl",
    argumentSensitive: false,
    capabilities: ["seedsSpawnTriggeringState"],
    pins: [],
  },
  "list_pending_permission_gates": {
    opClass: "read",
    argumentSensitive: false,
    capabilities: [],
    pins: [],
  },
  "list_pending_question_gates": {
    opClass: "read",
    argumentSensitive: false,
    capabilities: [],
    pins: [],
  },
  "list_tasks": {
    opClass: "read",
    argumentSensitive: false,
    capabilities: [],
    pins: [],
  },
  "move_task": {
    opClass: "agentControl",
    argumentSensitive: false,
    capabilities: ["agentControl","mutatesAgentConsumedContent"],
    pins: [],
  },
  "pause_execution": {
    opClass: "operate",
    argumentSensitive: false,
    capabilities: [],
    pins: [],
  },
  "pause_task": {
    opClass: "operate",
    argumentSensitive: false,
    capabilities: [],
    pins: [],
  },
  "pause_tasks_in_group": {
    opClass: "operate",
    argumentSensitive: false,
    capabilities: [],
    pins: [],
  },
  "reanalyze_project": {
    opClass: "agentControl",
    argumentSensitive: false,
    capabilities: ["agentControl"],
    pins: [],
  },
  "resume_automation": {
    opClass: "agentControl",
    argumentSensitive: false,
    capabilities: ["seedsSpawnTriggeringState"],
    pins: [],
  },
  "search_tasks": {
    opClass: "read",
    argumentSensitive: false,
    capabilities: [],
    pins: [],
  },
  "send_remote_chat_message": {
    opClass: "agentControl",
    argumentSensitive: false,
    capabilities: ["mutatesAgentConsumedContent"],
    pins: [{"param":"input","field":"role","value":"user"}],
  },
  "stop_execution": {
    opClass: "operate",
    argumentSensitive: false,
    capabilities: [],
    pins: [],
  },
  "stop_task": {
    opClass: "operate",
    argumentSensitive: false,
    capabilities: [],
    pins: [],
  },
  "unblock_task": {
    opClass: "agentControl",
    argumentSensitive: false,
    capabilities: ["agentControl"],
    pins: [],
  },
  "update_artifact": {
    opClass: "agentControl",
    argumentSensitive: false,
    capabilities: ["mutatesAgentConsumedContent"],
    pins: [],
  },
  "update_task": {
    opClass: "operate",
    argumentSensitive: true,
    capabilities: [],
    pins: [],
  },
  "update_task_proposal": {
    opClass: "agentControl",
    argumentSensitive: false,
    capabilities: ["mutatesAgentConsumedContent"],
    pins: [],
  },
  "update_task_step": {
    opClass: "agentControl",
    argumentSensitive: false,
    capabilities: ["mutatesAgentConsumedContent"],
    pins: [],
  },
};

export interface RemoteConditionalCapability {
  readonly capability: string;
  /** Argument fields that escalate the op's effective class to `agentControl`. */
  readonly fields: readonly string[];
}

/** Ops whose required scope depends on WHICH fields the caller is changing. */
export const REMOTE_CONDITIONAL_CAPABILITIES: Readonly<
  Record<string, RemoteConditionalCapability>
> = {
  "update_task": {
    capability: "mutatesAgentConsumedContent",
    fields: ["title","description"],
  },
};
