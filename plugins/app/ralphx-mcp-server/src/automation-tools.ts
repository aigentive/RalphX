import { Tool } from "@modelcontextprotocol/sdk/types.js";
import type { TauriCallOptions } from "./tauri-client.js";

type TauriPost = (
  path: string,
  body: Record<string, unknown>,
  options?: TauriCallOptions
) => Promise<unknown>;

export type AutomationSetupToolRuntimeContext = {
  conversationId?: string;
};

export const AUTOMATION_SETUP_TOOLS: Tool[] = [
  {
    name: "get_automation",
    description:
      "Read the automation record and run list bound to the current automation setup conversation. " +
      "The backend resolves ownership from the caller conversation; do not pass an automation id.",
    inputSchema: {
      type: "object",
      properties: {},
      required: [],
    },
  },
  {
    name: "update_automation",
    description:
      "Update the settings and configuration of the draft (or paused) automation bound to the " +
      "current setup conversation. Every field is optional; only provided fields are written. " +
      "The backend resolves ownership from the caller conversation; do not pass an automation id.",
    inputSchema: {
      type: "object",
      properties: {
        name: {
          type: "string",
          description: "Optional automation display name.",
        },
        max_runs: {
          type: "integer",
          description: "Optional positive maximum number of automation runs.",
        },
        max_consecutive_failures: {
          type: "integer",
          description:
            "Optional positive maximum number of consecutive failures before pausing.",
        },
        plan_approval_mode: {
          type: "string",
          enum: ["manual", "automatic"],
          description:
            "Plan approval mode. Use 'automatic' to let the plan gate proceed after successful judge approval.",
        },
        pr_merge_mode: {
          type: "string",
          enum: ["manual", "automatic"],
          description:
            "PR merge mode. Use 'automatic' to request native GitHub auto-merge for published run PRs.",
        },
        plan_deep_verification: {
          type: "boolean",
          description:
            "Enable deeper plan verification before an approved plan proceeds. Required for the ideation task-graph bridge.",
        },
        goal_prompt: {
          type: "string",
          description:
            "Durable goal for the automation. Required (non-empty) before finalize.",
        },
        first_run_prompt: {
          type: "string",
          description:
            "Self-contained prompt for run 1 instructing the agent to produce the configured PR or verified task-graph deliverable. Required before finalize.",
        },
        provider_harness: {
          type: "string",
          description: "Provider harness for the run agent (e.g. 'claude' or 'codex').",
        },
        model_id: {
          type: "string",
          description: "Model id for the run agent (e.g. 'sonnet').",
        },
        logical_effort: {
          type: "string",
          description: "Optional logical effort hint for the run agent.",
        },
        run_mode: {
          type: "string",
          enum: ["edit", "ideation"],
          description:
            "Run deliverable: 'edit' publishes a PR; 'ideation' turns a verified plan into proposals, task dependencies, and the local task pipeline.",
        },
        base_ref_kind: {
          type: "string",
          description:
            "Base ref kind: 'project_default' or 'local_branch' (a resolved branch or PR base).",
        },
        base_ref: {
          type: "string",
          description:
            "Base ref value. Required and non-empty when base_ref_kind is 'local_branch'.",
        },
        base_display_name: {
          type: "string",
          description: "Optional human-readable base label shown in the UI.",
        },
        goal_items_json: {
          type: "string",
          description:
            "Optional JSON array of automation phases or goal items. Use stable item ids and status values such as pending, in_progress, done, or skipped.",
        },
        chain_mode: {
          type: "string",
          description: "Successor chaining mode (e.g. 'merged_base').",
        },
        completion_signal: {
          type: "string",
          enum: ["pr_merged", "agent_completed", "ideation_finalized"],
          description:
            "Completion signal. Use 'pr_merged' for edit runs and 'ideation_finalized' for the ideation task-graph bridge.",
        },
        setup_analysis_summary: {
          type: "string",
          description: "Optional concise summary of the setup analysis (assumptions/constraints only).",
        },
        spec_content: {
          type: "string",
          description:
            "Full automation spec markdown. When provided, the backend persists it as a Specification artifact and links it (re-authoring creates a new version). Author or load the spec first, then derive goal_prompt, goal_items_json phases, and first_run_prompt from it.",
        },
        spec_artifact_id: {
          type: "string",
          description:
            "Link an existing Specification artifact (e.g. an ideation/handoff spec) as this automation's spec. The artifact must already exist. Prefer spec_content when authoring new spec markdown.",
        },
      },
      required: [],
    },
  },
  {
    name: "verify_automation_decomposition",
    description:
      "Run the independent decomposition-quality verifier for the trusted auto-finalize automation bound to this setup conversation. " +
      "A verified current decomposition is finalized automatically; a revise verdict leaves the draft editable and returns actionable findings. " +
      "The backend resolves ownership from the caller conversation; do not pass an automation id.",
    inputSchema: {
      type: "object",
      properties: {},
      required: [],
    },
  },
  {
    name: "finalize_automation",
    description:
      "Mark the draft automation spec approved after backend validation passes. " +
      "The backend resolves ownership from the caller conversation; do not pass an automation id.",
    inputSchema: {
      type: "object",
      properties: {},
      required: [],
    },
  },
];

const AUTOMATION_SETUP_TOOL_NAMES = new Set(
  AUTOMATION_SETUP_TOOLS.map((tool) => tool.name)
);

const CALLER_SESSION_ID_HEADER = "X-RalphX-Caller-Session-Id";
const UPDATE_AUTOMATION_FIELDS = [
  "name",
  "max_runs",
  "max_consecutive_failures",
  "plan_approval_mode",
  "pr_merge_mode",
  "plan_deep_verification",
  "goal_prompt",
  "first_run_prompt",
  "provider_harness",
  "model_id",
  "logical_effort",
  "run_mode",
  "base_ref_kind",
  "base_ref",
  "base_display_name",
  "goal_items_json",
  "chain_mode",
  "completion_signal",
  "setup_analysis_summary",
  "spec_content",
  "spec_artifact_id",
] as const;

export function isAutomationSetupToolName(name: string): boolean {
  return AUTOMATION_SETUP_TOOL_NAMES.has(name);
}

export async function callAutomationSetupTool(
  name: string,
  callTauri: TauriPost,
  args: unknown,
  runtimeContext?: AutomationSetupToolRuntimeContext
): Promise<unknown> {
  const headers = automationSetupHeaders(name, runtimeContext);

  switch (name) {
    case "get_automation":
      return callTauri("get_automation", {}, { headers });
    case "update_automation":
      return callTauri(
        "update_automation",
        updateAutomationPayload(args),
        { headers }
      );
    case "verify_automation_decomposition":
      return callTauri("verify_automation_decomposition", {}, { headers });
    case "finalize_automation":
      return callTauri("finalize_automation", {}, { headers });
    default:
      throw new Error(`Unsupported automation setup tool: ${name}`);
  }
}

function automationSetupHeaders(
  toolName: string,
  runtimeContext?: AutomationSetupToolRuntimeContext
): Record<string, string> {
  const conversationId = runtimeContext?.conversationId?.trim() ?? "";
  if (conversationId.length === 0) {
    throw new Error(
      `${toolName} requires the current setup conversation id from the RalphX MCP runtime context.`
    );
  }

  return {
    [CALLER_SESSION_ID_HEADER]: conversationId,
  };
}

function updateAutomationPayload(args: unknown): Record<string, unknown> {
  const input = args && typeof args === "object"
    ? (args as Record<string, unknown>)
    : {};
  const payload: Record<string, unknown> = {};

  for (const field of UPDATE_AUTOMATION_FIELDS) {
    if (Object.prototype.hasOwnProperty.call(input, field)) {
      payload[field] = input[field];
    }
  }

  return payload;
}
