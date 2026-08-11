const EMPTY_CALLER_BOUND_INPUT_SCHEMA = {
    type: "object",
    properties: {},
    required: [],
};
function callerBoundAutomationTool(name, description) {
    return {
        name,
        description: description +
            " The backend resolves the automation and current run or workspace from the caller conversation; do not pass ids.",
        inputSchema: EMPTY_CALLER_BOUND_INPUT_SCHEMA,
    };
}
export const AUTOMATION_SETUP_TOOLS = [
    {
        name: "get_automation",
        description: "Read the automation record and run list bound to the current automation setup conversation. " +
            "The backend resolves ownership from the caller conversation; do not pass an automation id.",
        inputSchema: {
            type: "object",
            properties: {},
            required: [],
        },
    },
    {
        name: "update_automation",
        description: "Update the settings and configuration of the draft (or paused) automation bound to the " +
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
                    description: "Optional positive maximum number of consecutive failures before pausing.",
                },
                plan_approval_mode: {
                    type: "string",
                    enum: ["manual", "automatic"],
                    description: "Plan approval mode. Use 'automatic' to let the plan gate proceed after successful judge approval.",
                },
                pr_merge_mode: {
                    type: "string",
                    enum: ["manual", "automatic"],
                    description: "PR merge mode. Use 'automatic' to request native GitHub auto-merge for published run PRs.",
                },
                plan_deep_verification: {
                    type: "boolean",
                    description: "Enable deeper plan verification before an approved plan proceeds. Required for the ideation task-graph bridge.",
                },
                goal_prompt: {
                    type: "string",
                    description: "Durable goal for the automation. Required (non-empty) before finalize.",
                },
                first_run_prompt: {
                    type: "string",
                    description: "Self-contained prompt for run 1 instructing the agent to produce the configured PR or verified task-graph deliverable. Required before finalize.",
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
                    description: "Run deliverable: 'edit' publishes a PR; 'ideation' turns a verified plan into proposals, task dependencies, and the local task pipeline.",
                },
                base_ref_kind: {
                    type: "string",
                    description: "Base ref kind: 'project_default' or 'local_branch' (a resolved branch or PR base).",
                },
                base_ref: {
                    type: "string",
                    description: "Base ref value. Required and non-empty when base_ref_kind is 'local_branch'.",
                },
                base_display_name: {
                    type: "string",
                    description: "Optional human-readable base label shown in the UI.",
                },
                goal_items_json: {
                    type: "string",
                    description: "Optional JSON array of automation phases or goal items. Use stable item ids and status values such as pending, in_progress, done, or skipped.",
                },
                chain_mode: {
                    type: "string",
                    description: "Successor chaining mode (e.g. 'merged_base').",
                },
                completion_signal: {
                    type: "string",
                    enum: ["pr_merged", "agent_completed", "ideation_finalized"],
                    description: "Completion signal. Use 'pr_merged' for edit runs and 'ideation_finalized' for the ideation task-graph bridge.",
                },
                setup_analysis_summary: {
                    type: "string",
                    description: "Optional concise summary of the setup analysis (assumptions/constraints only).",
                },
                spec_content: {
                    type: "string",
                    description: "Full automation spec markdown. When provided, the backend persists it as a Specification artifact and links it (re-authoring creates a new version). Author or load the spec first, then derive goal_prompt, goal_items_json phases, and first_run_prompt from it.",
                },
                spec_artifact_id: {
                    type: "string",
                    description: "Link an existing Specification artifact (e.g. an ideation/handoff spec) as this automation's spec. The artifact must already exist. Prefer spec_content when authoring new spec markdown.",
                },
            },
            required: [],
        },
    },
    {
        name: "verify_automation_decomposition",
        description: "Run the independent decomposition-quality verifier for the trusted auto-finalize automation bound to this setup conversation. " +
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
        description: "Mark the draft automation spec approved after backend validation passes. " +
            "The backend resolves ownership from the caller conversation; do not pass an automation id.",
        inputSchema: {
            type: "object",
            properties: {},
            required: [],
        },
    },
    callerBoundAutomationTool("run_automation_now", "Start a fresh run for the active automation. If the latest run was cancelled, the new run reuses its durable prompt; it never revives the cancelled run."),
    callerBoundAutomationTool("pause_automation", "Pause automatic scheduling without cancelling the automation. Use resume_automation to continue scheduling later."),
    callerBoundAutomationTool("resume_automation", "Resume scheduling for a paused automation. This does not revive a cancelled run; use run_automation_now when fresh work is needed."),
    callerBoundAutomationTool("cancel_automation_run", "Cancel the latest open run while leaving the automation active. Completed work and artifacts remain inspectable, and a later run must be fresh."),
    callerBoundAutomationTool("cancel_automation", "Cancel the automation: cancel open runs and disable automatic scheduling while preserving conversations, artifacts, branches, PRs, and completed work. Use restart_automation for a fresh run later."),
    callerBoundAutomationTool("restart_automation", "Reactivate a stopped automation and create a fresh run from its durable configuration and latest prompt. This never resumes a cancelled process or run row."),
    callerBoundAutomationTool("retry_automation_judge", "Retry the terminal judge only when the latest signal-terminal run has a persisted failed judge state. The backend rejects stale, ineligible, or already-running attempts."),
    callerBoundAutomationTool("retry_automation_plan_judge", "Retry the plan judge only when the parked latest run and exact current plan artifact have a persisted failed plan-judge state. The backend rejects stale attempts."),
    callerBoundAutomationTool("skip_automation_judge", "Skip the recoverable terminal judge only when the automation chain mode and latest-run state support it."),
    callerBoundAutomationTool("get_automation_publish_status", "Read Commit & Publish status for the publishable setup or latest eligible run workspace selected by RalphX."),
    callerBoundAutomationTool("check_automation_publish_readiness", "Check base freshness, local changes, and publish readiness for the publishable automation workspace selected by RalphX."),
    callerBoundAutomationTool("update_automation_from_base", "Update the publishable automation workspace from its configured base through the existing workspace recovery pipeline."),
    callerBoundAutomationTool("publish_automation_workspace", "Publish the selected automation workspace through RalphX's existing Commit & Publish pipeline. Call only after the user explicitly asks to commit, publish, or open a PR."),
];
const AUTOMATION_SETUP_TOOL_NAMES = new Set(AUTOMATION_SETUP_TOOLS.map((tool) => tool.name));
const CALLER_SESSION_ID_HEADER = "X-RalphX-Caller-Session-Id";
const CALLER_BOUND_ACTION_PATHS = {
    run_automation_now: "run_automation_now",
    pause_automation: "pause_automation",
    resume_automation: "resume_automation",
    cancel_automation_run: "cancel_automation_run",
    cancel_automation: "cancel_automation",
    restart_automation: "restart_automation",
    retry_automation_judge: "retry_automation_judge",
    retry_automation_plan_judge: "retry_automation_plan_judge",
    skip_automation_judge: "skip_automation_judge",
    get_automation_publish_status: "get_automation_publish_status",
    check_automation_publish_readiness: "check_automation_publish_readiness",
    update_automation_from_base: "update_automation_from_base",
    publish_automation_workspace: "publish_automation_workspace",
};
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
];
export function isAutomationSetupToolName(name) {
    return AUTOMATION_SETUP_TOOL_NAMES.has(name);
}
export async function callAutomationSetupTool(name, callTauri, args, runtimeContext) {
    const headers = automationSetupHeaders(name, runtimeContext);
    const callerBoundPath = CALLER_BOUND_ACTION_PATHS[name];
    if (callerBoundPath) {
        return callTauri(callerBoundPath, {}, { headers });
    }
    switch (name) {
        case "get_automation":
            return callTauri("get_automation", {}, { headers });
        case "update_automation":
            return callTauri("update_automation", updateAutomationPayload(args), { headers });
        case "verify_automation_decomposition":
            return callTauri("verify_automation_decomposition", {}, { headers });
        case "finalize_automation":
            return callTauri("finalize_automation", {}, { headers });
        default:
            throw new Error(`Unsupported automation setup tool: ${name}`);
    }
}
function automationSetupHeaders(toolName, runtimeContext) {
    const conversationId = runtimeContext?.conversationId?.trim() ?? "";
    if (conversationId.length === 0) {
        throw new Error(`${toolName} requires the current setup conversation id from the RalphX MCP runtime context.`);
    }
    return {
        [CALLER_SESSION_ID_HEADER]: conversationId,
    };
}
function updateAutomationPayload(args) {
    const input = args && typeof args === "object"
        ? args
        : {};
    const payload = {};
    for (const field of UPDATE_AUTOMATION_FIELDS) {
        if (Object.prototype.hasOwnProperty.call(input, field)) {
            payload[field] = input[field];
        }
    }
    return payload;
}
//# sourceMappingURL=automation-tools.js.map