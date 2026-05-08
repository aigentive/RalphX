#!/usr/bin/env node
/**
 * RalphX MCP Server
 *
 * A proxy MCP server that forwards tool calls to the RalphX Tauri backend via HTTP.
 * All business logic lives in Rust - this server is a thin transport layer.
 *
 * Tool scoping:
 * - Reads agent type from CLI args (--agent-type=<type>) or environment (RALPHX_AGENT_TYPE)
 * - CLI args take precedence (because Claude CLI doesn't pass env vars to MCP servers)
 * - Filters available tools based on agent type (hard enforcement)
 * - Each agent only sees tools appropriate for its role
 */
import { Server } from "@modelcontextprotocol/sdk/server/index.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { CallToolRequestSchema, ListToolsRequestSchema, } from "@modelcontextprotocol/sdk/types.js";
import { callTauri, callTauriGet, TauriClientError } from "./tauri-client.js";
import { getTraceLogPath, safeError, safeTrace } from "./redact.js";
import { getFilteredTools, isToolAllowed, getAllowedToolNames, parseAllowedToolsFromArgs, formatToolErrorMessage, logAllTools, getToolsByAgent, setAgentType, } from "./tools.js";
import { FILESYSTEM_TOOL_NAMES, formatFilesystemToolError, handleFilesystemToolCall, } from "./filesystem-tools.js";
import { permissionRequestTool, handlePermissionRequest, } from "./permission-handler.js";
import { handleAskUserQuestion } from "./question-handler.js";
import { handleRequestTeamPlan } from "./team-plan-handler.js";
import { hydrateRalphxRuntimeEnvFromCli, parseCliOptionFromArgs, } from "./runtime-context.js";
import { createVerificationRuntime } from "./verification-runtime.js";
import { buildAppendTaskToIdeationPlanPayload } from "./append-task-payload.js";
import { callCompleteAgentWorkspaceRepairTool, callSubmitAgentWorkspacePrDescriptionTool, } from "./agent-workspace-tools.js";
/**
 * Semantic keyword patterns for cross-project detection in plan text.
 * Exported for unit testing.
 */
export const CROSS_PROJECT_KEYWORDS = [
    "cross[- ]?project",
    "multi[- ]?project",
    "target project",
    "another project",
    "different project",
    "project[_ ]?b\\b",
    "separate\\s+repo(?:sitory)?",
    "new\\s+repo(?:sitory)?",
    "different\\s+codebase",
    "other\\s+codebase",
    "monorepo\\s+boundary",
    "external\\s+package",
    "external\\s+module",
];
/**
 * Strip fenced and inline markdown code blocks from text before path scanning.
 * Prevents false-positive path detection on code snippets like `...>>` or `...`.
 * Exported for unit testing.
 */
export function stripMarkdownCodeBlocks(text) {
    // Remove fenced code blocks (``` ... ```) — non-greedy, handles multi-line
    let stripped = text.replace(/```[\s\S]*?```/g, "");
    // Remove inline code (`...`)
    stripped = stripped.replace(/`[^`\n]+`/g, "");
    return stripped;
}
/**
 * Filter out detected paths that belong to the same project root.
 * Returns only paths that genuinely reference a different project.
 *
 * @param detectedPaths - Raw list of absolute or relative paths found in plan text
 * @param projectWorkingDir - The project's working directory (e.g. /Users/alice/Code/ralphx)
 * @returns Paths that do NOT start with projectWorkingDir (i.e. are truly cross-project)
 */
export function filterCrossProjectPaths(detectedPaths, projectWorkingDir) {
    if (!projectWorkingDir) {
        return detectedPaths;
    }
    // Normalize: ensure root ends with exactly one slash for prefix matching
    const root = projectWorkingDir.endsWith("/")
        ? projectWorkingDir
        : projectWorkingDir + "/";
    return detectedPaths.filter((p) => {
        // Exact match: path equals project root (without trailing slash)
        if (p === projectWorkingDir)
            return false;
        // Prefix match: path is inside project root
        if (p.startsWith(root))
            return false;
        return true;
    });
}
function summarizeResult(result) {
    if (result === null) {
        return { kind: "null" };
    }
    if (result === undefined) {
        return { kind: "undefined" };
    }
    if (typeof result === "string") {
        return { kind: "string", length: result.length };
    }
    if (typeof result === "number" || typeof result === "boolean") {
        return { kind: typeof result, value: result };
    }
    if (Array.isArray(result)) {
        return { kind: "array", length: result.length };
    }
    if (typeof result === "object") {
        return {
            kind: "object",
            keys: Object.keys(result).slice(0, 20),
        };
    }
    return { kind: typeof result };
}
const runtimeContext = hydrateRalphxRuntimeEnvFromCli(process.argv, process.env);
const cliAgentType = parseCliOptionFromArgs(process.argv, "agent-type");
// Agent type: prefer CLI args over environment and hydrate process.env from CLI first
// because Codex does not reliably propagate parent env vars into MCP child processes.
const AGENT_TYPE = runtimeContext.agentType || "unknown";
// Set the agent type in tools module for filtering
setAgentType(AGENT_TYPE);
// Log how agent type was determined
if (cliAgentType) {
    safeError(`[RalphX MCP] Agent type from CLI args: ${AGENT_TYPE}`);
}
else if (process.env.RALPHX_AGENT_TYPE) {
    safeError(`[RalphX MCP] Agent type from env: ${AGENT_TYPE}`);
}
else {
    safeError(`[RalphX MCP] Agent type unknown (no CLI arg or env var)`);
}
// Runtime scope for task/project/context enforcement.
const RALPHX_TASK_ID = runtimeContext.taskId;
const RALPHX_PROJECT_ID = runtimeContext.projectId;
const RALPHX_WORKING_DIRECTORY = runtimeContext.workingDirectory;
const RALPHX_CONTEXT_TYPE = runtimeContext.contextType;
const RALPHX_CONTEXT_ID = runtimeContext.contextId;
function buildArtifactMutationTransportHeaders() {
    if (RALPHX_CONTEXT_TYPE !== "ideation" || !RALPHX_CONTEXT_ID) {
        return undefined;
    }
    return {
        "X-RalphX-Caller-Session-Id": RALPHX_CONTEXT_ID,
    };
}
const { getPlanVerificationForTool, reportVerificationRoundForTool, completePlanVerificationForTool, runVerificationEnrichment, runVerificationRound, resolveVerificationFindingSessionId, resolveContextSessionId, } = createVerificationRuntime({
    callTauri,
    callTauriGet,
    agentType: AGENT_TYPE,
    contextType: RALPHX_CONTEXT_TYPE,
    contextId: RALPHX_CONTEXT_ID,
});
/**
 * Validate that a tool call's task_id parameter matches the assigned task
 * @param toolName - Name of the tool being called
 * @param args - Arguments passed to the tool
 * @returns Error message if validation fails, null if validation passes or not applicable
 *
 * Test Cases:
 * 1. Non-scoped tool (get_artifact) => returns null (no validation)
 * 2. Scoped tool, no RALPHX_TASK_ID set => returns null (backward compat)
 * 3. Scoped tool, matching task_id => returns null (validation passed)
 * 4. Scoped tool, mismatched task_id => returns error message
 */
function validateTaskScope(toolName, args) {
    // Only validate tools that have task_id parameter directly
    // Note: start_step, complete_step, skip_step, fail_step take step_id, not task_id
    // The backend validates step ownership - we can't do it here without a DB lookup
    const taskScopedTools = [
        "complete_review",
        "approve_task",
        "request_task_changes",
        "update_task",
        "add_task_note",
        "get_task_details",
        "get_task_context",
        "get_review_notes",
        "get_task_steps",
        "add_step",
        "get_step_progress",
        // Merge tools (merger agent)
        "report_conflict",
        "report_incomplete",
        "complete_merge",
        "get_merge_target",
        // Issue tools (worker + reviewer agents)
        "get_task_issues",
        "get_issue_progress",
        // Execution complete (worker agent)
        "execution_complete",
    ];
    if (!taskScopedTools.includes(toolName)) {
        return null; // No validation needed
    }
    if (!RALPHX_TASK_ID) {
        return null; // No task scope set, allow (backward compatibility)
    }
    const providedTaskId = args.task_id;
    if (providedTaskId !== RALPHX_TASK_ID) {
        return `ERROR: Task scope violation.\n\nYou are assigned to task "${RALPHX_TASK_ID}" but attempted to modify task "${providedTaskId}".\n\nYour assigned task details:\n- Task ID: ${RALPHX_TASK_ID}\n- You should only call ${toolName} with this task_id.\n\nPlease correct your tool call and try again.`;
    }
    return null; // Validation passed
}
/**
 * Validate that a tool call's project_id parameter matches the assigned project
 * @param toolName - Name of the tool being called
 * @param args - Arguments passed to the tool
 * @returns Error message if validation fails, null if validation passes or not applicable
 */
function validateProjectScope(toolName, args) {
    const projectScopedTools = [
        "get_project_analysis",
        "save_project_analysis",
        "append_task_to_ideation_plan",
        // Memory write tools (memory agents only)
        // Note: mark_memory_obsolete excluded - uses memory_id lookup for implicit project validation
        "upsert_memories",
        "refresh_memory_rule_index",
        "ingest_rule_file",
        "rebuild_archive_snapshots",
    ];
    if (!projectScopedTools.includes(toolName)) {
        return null;
    }
    if (!RALPHX_PROJECT_ID) {
        return null; // No project scope set, allow (backward compatibility)
    }
    const providedProjectId = args.project_id;
    if (providedProjectId !== RALPHX_PROJECT_ID) {
        return `ERROR: Project scope violation.\n\nYou are assigned to project "${RALPHX_PROJECT_ID}" but attempted to access project "${providedProjectId}".\n\nPlease correct your tool call and try again.`;
    }
    return null;
}
/**
 * Create and configure the MCP server
 */
const server = new Server({
    name: "ralphx",
    version: "1.0.0",
}, {
    capabilities: {
        tools: {},
    },
});
/**
 * List available tools (filtered by agent type)
 * Note: permission_request tool is always included (not scoped by agent type)
 */
server.setRequestHandler(ListToolsRequestSchema, async () => {
    // Parse once — reuse for logging and to avoid a redundant argv scan inside getAllowedToolNames()
    const cliToolsArg = parseAllowedToolsFromArgs();
    const tools = getFilteredTools();
    // Always include permission_request tool (not scoped by agent type)
    const allTools = [...tools, permissionRequestTool];
    // Log tool scoping for debugging
    if (cliToolsArg !== undefined) {
        safeError(`[RalphX MCP] Tools from --allowed-tools: ${cliToolsArg.length > 0 ? cliToolsArg.join(", ") : "none (explicit __NONE__)"}`);
    }
    const toolNames = tools.map((t) => t.name);
    safeError(`[RalphX MCP] Agent type: ${AGENT_TYPE}, Tools: ${toolNames.length > 0 ? toolNames.join(", ") : "none"} + permission_request`);
    safeTrace("tools.list", {
        agent_type: AGENT_TYPE,
        tools: toolNames,
        includes_permission_request: true,
    });
    return { tools: allTools };
});
/**
 * Execute tool calls (with authorization check)
 */
server.setRequestHandler(CallToolRequestSchema, async (request) => {
    const { name, arguments: args } = request.params;
    safeTrace("tool.request", { name, args });
    // Special handling for permission_request tool (always allowed, not scoped by agent type)
    if (name === "permission_request") {
        try {
            const result = await handlePermissionRequest(args);
            safeTrace("tool.success", {
                name,
                result: summarizeResult(result),
            });
            return result;
        }
        catch (error) {
            safeTrace("tool.error", {
                name,
                error: error instanceof Error ? error.message : String(error),
            });
            const message = error instanceof Error ? error.message : String(error);
            return {
                content: [{ type: "text", text: JSON.stringify({ behavior: "deny", message }) }],
            };
        }
    }
    if (FILESYSTEM_TOOL_NAMES.includes(name)) {
        if (!isToolAllowed(name)) {
            return {
                content: [
                    {
                        type: "text",
                        text: `ERROR: Tool "${name}" is not available for agent type "${AGENT_TYPE}".`,
                    },
                ],
                isError: true,
            };
        }
        try {
            const result = await handleFilesystemToolCall(name, args);
            safeTrace("tool.success", {
                name,
                result: summarizeResult(result),
            });
            return result;
        }
        catch (error) {
            safeTrace("tool.error", {
                name,
                error: error instanceof Error ? error.message : String(error),
            });
            return formatFilesystemToolError(error);
        }
    }
    // Special handling for ask_user_question (register + long-poll, like permission_request)
    if (name === "ask_user_question") {
        // Still check authorization (must be in agent's allowlist)
        if (!isToolAllowed(name)) {
            return {
                content: [
                    {
                        type: "text",
                        text: `ERROR: Tool "${name}" is not available for agent type "${AGENT_TYPE}".`,
                    },
                ],
                isError: true,
            };
        }
        try {
            const result = await handleAskUserQuestion(args);
            safeTrace("tool.success", {
                name,
                result: summarizeResult(result),
            });
            return result;
        }
        catch (error) {
            safeTrace("tool.error", {
                name,
                error: error instanceof Error ? error.message : String(error),
            });
            const message = error instanceof Error ? error.message : String(error);
            return {
                content: [{ type: "text", text: `ERROR: Unexpected error: ${message}` }],
                isError: true,
            };
        }
    }
    // Special handling for request_team_plan (two-phase: register POST + long-poll GET)
    if (name === "request_team_plan") {
        // Still check authorization (must be in agent's allowlist)
        if (!isToolAllowed(name)) {
            return {
                content: [
                    {
                        type: "text",
                        text: `ERROR: Tool "${name}" is not available for agent type "${AGENT_TYPE}".`,
                    },
                ],
                isError: true,
            };
        }
        const leadSessionId = globalThis.process.env.RALPHX_LEAD_SESSION_ID;
        try {
            const result = await handleRequestTeamPlan(args, RALPHX_CONTEXT_TYPE ?? "ideation", RALPHX_CONTEXT_ID ?? "", leadSessionId);
            safeTrace("tool.success", {
                name,
                result: summarizeResult(result),
            });
            return result;
        }
        catch (error) {
            safeTrace("tool.error", {
                name,
                error: error instanceof Error ? error.message : String(error),
            });
            const message = error instanceof Error ? error.message : String(error);
            return {
                content: [{ type: "text", text: `ERROR: Unexpected error: ${message}` }],
                isError: true,
            };
        }
    }
    // Authorization check (defense in depth)
    if (!isToolAllowed(name)) {
        const allowedNames = getAllowedToolNames();
        const errorMessage = allowedNames.length > 0
            ? `Tool "${name}" is not available for agent type "${AGENT_TYPE}". Allowed tools: ${allowedNames.join(", ")}`
            : `Agent type "${AGENT_TYPE}" has no MCP tools available. This agent should use filesystem tools (Read, Grep, Glob, Bash, Edit, Write) instead.`;
        safeError(`[RalphX MCP] Unauthorized tool call: ${name}`);
        safeTrace("tool.denied", { name, reason: "unauthorized" });
        return {
            content: [
                {
                    type: "text",
                    text: `ERROR: ${errorMessage}`,
                },
            ],
            isError: true,
        };
    }
    // Task scope validation
    const scopeError = validateTaskScope(name, args || {});
    if (scopeError) {
        safeError(`[RalphX MCP] Task scope violation: ${name}`);
        safeTrace("tool.denied", { name, reason: "task_scope_violation" });
        return {
            content: [
                {
                    type: "text",
                    text: scopeError,
                },
            ],
            isError: true,
        };
    }
    // Project scope validation
    const projectScopeError = validateProjectScope(name, args || {});
    if (projectScopeError) {
        safeError(`[RalphX MCP] Project scope violation: ${name}`);
        safeTrace("tool.denied", { name, reason: "project_scope_violation" });
        return {
            content: [
                {
                    type: "text",
                    text: projectScopeError,
                },
            ],
            isError: true,
        };
    }
    try {
        // Forward to Tauri backend
        safeError(`[RalphX MCP] Calling Tauri: ${name} with args:`, JSON.stringify(args));
        safeTrace("tool.dispatch", { name });
        let result;
        // Special handling for GET endpoints with path parameters
        if (name === "get_task_context") {
            const { task_id } = args;
            result = await callTauriGet(`task_context/${task_id}`);
        }
        else if (name === "get_artifact") {
            const { artifact_id } = args;
            result = await callTauriGet(`artifact/${artifact_id}`);
        }
        else if (name === "get_artifact_version") {
            const { artifact_id, version } = args;
            result = await callTauriGet(`artifact/${artifact_id}/version/${version}`);
        }
        else if (name === "get_related_artifacts") {
            const { artifact_id } = args;
            result = await callTauriGet(`artifact/${artifact_id}/related`);
        }
        else if (name === "get_plan_artifact") {
            // DEPRECATED: alias for backward compat — routes to get_artifact handler
            const { artifact_id } = args;
            result = await callTauriGet(`artifact/${artifact_id}`);
        }
        else if (name === "get_session_plan") {
            // Also handle get_session_plan as GET
            const { session_id } = args;
            result = await callTauriGet(`get_session_plan/${session_id}`);
        }
        else if (name === "update_plan_artifact" || name === "edit_plan_artifact") {
            const artifactMutationArgs = { ...(args ?? {}) };
            delete artifactMutationArgs.caller_session_id;
            result = await callTauri(name, artifactMutationArgs, {
                headers: buildArtifactMutationTransportHeaders(),
            });
        }
        else if (name === "get_plan_verification") {
            result = await getPlanVerificationForTool(args);
        }
        else if (name === "report_verification_round") {
            result = await reportVerificationRoundForTool(args);
        }
        else if (name === "run_verification_enrichment") {
            result = await runVerificationEnrichment(args);
        }
        else if (name === "run_verification_round") {
            result = await runVerificationRound(args);
        }
        else if (name === "complete_plan_verification") {
            result = await completePlanVerificationForTool(args);
        }
        else if (name === "revert_and_skip") {
            // POST /api/ideation/sessions/:id/revert-and-skip
            const { session_id, plan_version_to_restore } = args;
            result = await callTauri(`ideation/sessions/${session_id}/revert-and-skip`, { plan_version_to_restore });
        }
        else if (name === "stop_verification") {
            // POST /api/ideation/sessions/:id/stop-verification
            const { session_id } = args;
            result = await callTauri(`ideation/sessions/${session_id}/stop-verification`, {});
        }
        else if (name === "get_task_steps") {
            // GET /api/task_steps/:task_id
            const { task_id } = args;
            result = await callTauriGet(`task_steps/${task_id}`);
        }
        else if (name === "get_step_progress") {
            // GET /api/step_progress/:task_id
            const { task_id } = args;
            result = await callTauriGet(`step_progress/${task_id}`);
        }
        else if (name === "get_step_context") {
            // GET /api/step_context/:step_id
            const { step_id } = args;
            result = await callTauriGet(`step_context/${step_id}`);
        }
        else if (name === "get_sub_steps") {
            // GET /api/sub_steps/:parent_step_id
            const { parent_step_id } = args;
            result = await callTauriGet(`sub_steps/${parent_step_id}`);
        }
        else if (name === "get_review_notes") {
            // GET /api/review_notes/:task_id
            const { task_id } = args;
            result = await callTauriGet(`review_notes/${task_id}`);
        }
        else if (name === "list_session_proposals") {
            // GET /api/list_session_proposals/:session_id
            const { session_id } = args;
            result = await callTauriGet(`list_session_proposals/${session_id}`);
        }
        else if (name === "get_proposal") {
            // GET /api/proposal/:proposal_id
            const { proposal_id } = args;
            result = await callTauriGet(`proposal/${proposal_id}`);
        }
        else if (name === "analyze_session_dependencies") {
            // GET /api/analyze_dependencies/:session_id
            const { session_id } = args;
            result = await callTauriGet(`analyze_dependencies/${session_id}`);
        }
        else if (name === "complete_merge") {
            // POST /api/git/tasks/:task_id/complete-merge
            const { task_id, commit_sha } = args;
            result = await callTauri(`git/tasks/${task_id}/complete-merge`, { commit_sha });
        }
        else if (name === "complete_agent_workspace_repair") {
            // POST /api/agent-workspaces/:conversation_id/complete-repair
            result = await callCompleteAgentWorkspaceRepairTool(callTauri, args);
        }
        else if (name === "submit_agent_workspace_pr_description") {
            // POST /api/agent-workspaces/:conversation_id/pr-description
            result = await callSubmitAgentWorkspacePrDescriptionTool(callTauri, args);
        }
        else if (name === "report_conflict") {
            // POST /api/git/tasks/:task_id/report-conflict
            const { task_id, conflict_files, reason } = args;
            result = await callTauri(`git/tasks/${task_id}/report-conflict`, { conflict_files, reason });
        }
        else if (name === "report_incomplete") {
            // POST /api/git/tasks/:task_id/report-incomplete
            const { task_id, reason, diagnostic_info } = args;
            result = await callTauri(`git/tasks/${task_id}/report-incomplete`, { reason, diagnostic_info });
        }
        else if (name === "get_merge_target") {
            const { task_id } = args;
            result = await callTauriGet(`git/tasks/${task_id}/merge-target`);
        }
        else if (name === "get_task_issues") {
            // GET /api/task_issues/:task_id?status=<filter>
            const { task_id, status_filter } = args;
            const query = status_filter ? `?status=${status_filter}` : "";
            result = await callTauriGet(`task_issues/${task_id}${query}`);
        }
        else if (name === "get_issue_progress") {
            // GET /api/issue_progress/:task_id
            const { task_id } = args;
            result = await callTauriGet(`issue_progress/${task_id}`);
        }
        else if (name === "mark_issue_in_progress") {
            // POST /api/mark_issue_in_progress
            const { issue_id } = args;
            result = await callTauri("mark_issue_in_progress", { issue_id });
        }
        else if (name === "mark_issue_addressed") {
            // POST /api/mark_issue_addressed
            const { issue_id, resolution_notes, attempt_number } = args;
            result = await callTauri("mark_issue_addressed", { issue_id, resolution_notes, attempt_number });
        }
        else if (name === "create_child_session") {
            // POST /api/create_child_session
            const { parent_session_id, title, description, inherit_context, initial_prompt, team_mode, team_config, purpose } = args;
            // Propagate external trigger context from the spawning process env var.
            // RALPHX_IS_EXTERNAL_TRIGGER=1 is set by the backend when the agent was spawned
            // in response to an external MCP message (is_external_mcp=true).
            const is_external_trigger = process.env.RALPHX_IS_EXTERNAL_TRIGGER === "1";
            result = await callTauri("create_child_session", { parent_session_id, title, description, inherit_context, initial_prompt, team_mode, team_config, purpose, is_external_trigger });
        }
        else if (name === "create_followup_session") {
            // POST /api/create_child_session with first-class execution/review provenance
            const { source_ideation_session_id, title, description, inherit_context, initial_prompt, source_task_id, source_context_type, source_context_id, spawn_reason, blocker_fingerprint, } = args;
            let resolvedParentSessionId = source_ideation_session_id;
            let resolvedBlockerFingerprint = blocker_fingerprint;
            if (!resolvedParentSessionId && source_task_id) {
                const taskContext = await callTauriGet(`task_context/${source_task_id}`);
                resolvedParentSessionId = taskContext.task?.ideation_session_id ?? undefined;
                if (!resolvedBlockerFingerprint && spawn_reason === "out_of_scope_failure") {
                    resolvedBlockerFingerprint = taskContext.out_of_scope_blocker_fingerprint ?? undefined;
                }
            }
            if (!resolvedParentSessionId) {
                throw new Error("create_followup_session requires either source_ideation_session_id or a source_task_id that belongs to an ideation-backed task");
            }
            result = await callTauri("create_child_session", {
                parent_session_id: resolvedParentSessionId,
                title,
                description,
                inherit_context,
                initial_prompt,
                source_task_id,
                source_context_type,
                source_context_id,
                spawn_reason,
                blocker_fingerprint: resolvedBlockerFingerprint,
            });
        }
        else if (name === "get_parent_session_context") {
            // GET /api/parent_session_context/:session_id
            const { session_id } = args;
            result = await callTauriGet(`parent_session_context/${session_id}`);
        }
        else if (name === "delegate_start") {
            result = await callTauri("coordination/delegate/start", {
                ...args,
                caller_agent_name: AGENT_TYPE,
                caller_context_type: RALPHX_CONTEXT_TYPE,
                caller_context_id: RALPHX_CONTEXT_ID,
            });
        }
        else if (name === "delegate_wait") {
            result = await callTauri("coordination/delegate/wait", args);
        }
        else if (name === "delegate_cancel") {
            result = await callTauri("coordination/delegate/cancel", args);
        }
        else if (name === "get_project_analysis") {
            // GET /api/projects/:project_id/analysis?task_id=
            const { project_id, task_id } = args;
            const query = task_id ? `?task_id=${task_id}` : "";
            result = await callTauriGet(`projects/${project_id}/analysis${query}`);
        }
        else if (name === "save_project_analysis") {
            // POST /api/projects/:project_id/analysis
            const { project_id, entries } = args;
            result = await callTauri(`projects/${project_id}/analysis`, { entries });
        }
        else if (name === "request_teammate_spawn") {
            // POST /api/team/spawn
            const { role, prompt, model, tools, mcp_tools, preset } = args;
            result = await callTauri("team/spawn", { role, prompt, model, tools, mcp_tools, preset });
        }
        else if (name === "create_team_artifact") {
            // POST /api/team/artifact
            const { session_id, title, content, artifact_type, related_artifact_id } = args;
            result = await callTauri("team/artifact", {
                session_id,
                title,
                content,
                artifact_type,
                related_artifact_id,
            });
        }
        else if (name === "publish_verification_finding") {
            const { session_id, critic, round, status, coverage, summary, gaps, title_suffix, } = args;
            result = await callTauri("team/verification_finding", {
                session_id: await resolveVerificationFindingSessionId(session_id, "publish_verification_finding"),
                critic,
                round,
                status,
                coverage,
                summary,
                gaps,
                title_suffix,
            });
        }
        else if (name === "get_team_artifacts") {
            // GET /api/team/artifacts/:session_id
            const { session_id } = args;
            result = await callTauriGet(`team/artifacts/${session_id}`);
        }
        else if (name === "get_team_session_state") {
            // GET /api/team/session_state/:session_id
            const { session_id } = args;
            result = await callTauriGet(`team/session_state/${session_id}`);
        }
        else if (name === "save_team_session_state") {
            // POST /api/team/session_state
            const { session_id, team_composition, phase, artifact_ids } = args;
            result = await callTauri("team/session_state", {
                session_id,
                team_composition,
                phase,
                artifact_ids,
            });
        }
        else if (name === "execution_complete") {
            // POST /api/execution/tasks/:task_id/complete
            const { task_id, summary, test_result } = args;
            const body = { summary: summary || "" };
            if (test_result) {
                body.testResult = {
                    testsRan: test_result.tests_ran,
                    testsPassed: test_result.tests_passed,
                    testSummary: test_result.test_summary,
                };
            }
            result = await callTauri(`execution/tasks/${task_id}/complete`, body);
        }
        else if (name === "list_projects") {
            // GET /api/internal/projects
            result = await callTauriGet("internal/projects");
        }
        else if (name === "create_cross_project_session") {
            // POST /api/internal/cross_project/create_session
            const { target_project_path, source_session_id, title } = args;
            result = await callTauri("internal/cross_project/create_session", {
                targetProjectPath: target_project_path,
                sourceSessionId: source_session_id,
                title,
            });
        }
        else if (name === "migrate_proposals") {
            // POST /api/internal/cross_project/migrate_proposals
            const { source_session_id, target_session_id, proposal_ids, target_project_filter } = args;
            result = await callTauri("internal/cross_project/migrate_proposals", {
                sourceSessionId: source_session_id,
                targetSessionId: target_session_id,
                proposalIds: proposal_ids,
                targetProjectFilter: target_project_filter,
            });
        }
        else if (name === "cross_project_guide") {
            // MCP-server-only: analyze plan for cross-project paths and return guidance
            const { session_id, plan_content } = args;
            let planText = plan_content ?? "";
            let projectWorkingDir = null;
            // If session_id provided, fetch plan content via get_session_plan
            if (session_id && !planText) {
                try {
                    const planData = await callTauriGet(`get_session_plan/${session_id}`);
                    planText = planData?.content ?? "";
                    projectWorkingDir = planData?.project_working_directory ?? null;
                }
                catch (err) {
                    planText = "";
                    safeError(`[RalphX MCP] cross_project_guide: failed to fetch plan for session ${session_id}:`, err);
                }
            }
            // Strip code blocks before path scanning to avoid false positives on code snippets
            const scanText = stripMarkdownCodeBlocks(planText);
            // Heuristic: detect cross-project paths (absolute paths, ../relative paths, paths with project-like names)
            const crossProjectPatterns = [
                /(?:^|\s|["'`])(\/(home|Users|workspace|projects|srv|opt)\/[^\s"'`]+)/gm,
                /(?:^|\s|["'`])(\.\.\/[^\s"'`]+)/gm,
                /(?:target[_-]?project[_-]?path|project[_-]?path|working[_-]?directory)[:\s]+["']?([^\s"'`,\n]+)/gim,
            ];
            const rawDetectedPaths = [];
            for (const pattern of crossProjectPatterns) {
                const matches = [...scanText.matchAll(pattern)];
                for (const m of matches) {
                    const p = (m[1] || m[0]).trim().replace(/^["'`]|["'`]$/g, "");
                    if (p && !rawDetectedPaths.includes(p)) {
                        rawDetectedPaths.push(p);
                    }
                }
            }
            const detectedPaths = filterCrossProjectPaths(rawDetectedPaths, projectWorkingDir);
            const crossProjectKeywordRegex = new RegExp(CROSS_PROJECT_KEYWORDS.join("|"), "i");
            const hasCrossProjectContent = detectedPaths.length > 0 ||
                crossProjectKeywordRegex.test(planText);
            const analysisResult = {
                has_cross_project_paths: hasCrossProjectContent,
                detected_paths: detectedPaths,
                guidance: hasCrossProjectContent
                    ? {
                        summary: "This plan contains cross-project references. Follow these steps to orchestrate multi-project execution:",
                        steps: [
                            "1. Call list_projects to discover existing RalphX projects and their filesystem paths.",
                            "2. For each target project, call create_cross_project_session({ target_project_path, source_session_id }) to create a new session with the inherited plan.",
                            "3. In each target session, use create_task_proposal to create proposals specific to that project's scope.",
                            "4. Call accept_plan_and_schedule (or equivalent) in each target session to push tasks to kanban.",
                        ],
                        notes: [
                            "The target project is auto-created if no RalphX project exists at the given path.",
                            "The inherited plan is read-only in the target session. Call create_plan_artifact to create a writable copy if modifications are needed.",
                            "The inherited plan status is set to 'imported_verified' — no re-verification is triggered.",
                        ],
                        detected_paths: detectedPaths,
                    }
                    : {
                        summary: "No cross-project paths detected in this plan.",
                        steps: [],
                        notes: [
                            "If you believe there are cross-project references, try providing the plan_content directly or check the session_id.",
                        ],
                        detected_paths: [],
                    },
            };
            if (session_id) {
                try {
                    await callTauri(`internal/sessions/${session_id}/cross_project_check`, {});
                    result = { ...analysisResult, gate_status: "set" };
                }
                catch (err) {
                    const errMsg = err instanceof Error ? err.message : String(err);
                    safeError(`[RalphX MCP] cross_project_guide: failed to set gate for session ${session_id}:`, err);
                    result = {
                        ...analysisResult,
                        gate_status: "backend_unavailable",
                        gate_error: `Backend call failed: ${errMsg}`,
                    };
                }
            }
            else {
                result = {
                    ...analysisResult,
                    gate_status: "no_session_id",
                    gate_message: "Provide session_id to set the cross-project gate and unlock proposal creation",
                };
            }
        }
        else if (name === "get_child_session_status") {
            // GET /api/ideation/sessions/:id/child-status
            const { session_id, include_recent_messages, message_limit } = args;
            const params = new URLSearchParams();
            if (include_recent_messages)
                params.set("include_messages", "true");
            if (message_limit)
                params.set("message_limit", String(message_limit));
            const query = params.toString() ? `?${params}` : "";
            result = await callTauriGet(`ideation/sessions/${session_id}/child-status${query}`);
        }
        else if (name === "send_ideation_session_message") {
            // POST /api/ideation/sessions/:id/message
            const { session_id, message } = args;
            result = await callTauri(`ideation/sessions/${session_id}/message`, { message });
        }
        else if (name === "get_acceptance_status") {
            // GET /api/ideation/sessions/:id/acceptance-status
            const { session_id } = args;
            result = await callTauriGet(`ideation/sessions/${session_id}/acceptance-status`);
        }
        else if (name === "get_pending_confirmations") {
            // GET /api/ideation/pending-confirmations?project_id=xxx
            const projectId = RALPHX_PROJECT_ID;
            if (!projectId) {
                throw new Error("RALPHX_PROJECT_ID is not set — cannot query pending confirmations");
            }
            result = await callTauriGet(`ideation/pending-confirmations?project_id=${encodeURIComponent(projectId)}`);
        }
        else if (name === "get_verification_confirmation_status") {
            // GET /api/verification/confirmation-status/{session_id}
            const { session_id } = args;
            result = await callTauriGet(`verification/confirmation-status/${encodeURIComponent(session_id)}`);
        }
        else if (name === "delete_task_proposal") {
            // Alias for archive_task_proposal — no /api/delete_task_proposal route exists in backend
            const { proposal_id } = args;
            result = await callTauri("archive_task_proposal", { proposal_id });
        }
        else if (name === "append_task_to_ideation_plan") {
            result = await callTauri(name, buildAppendTaskToIdeationPlanPayload(args || {}));
        }
        else {
            // Default: POST request
            result = await callTauri(name, args || {});
        }
        safeError(`[RalphX MCP] Success: ${name}`);
        safeTrace("tool.success", {
            name,
            result: summarizeResult(result),
        });
        // Return result as JSON text
        return {
            content: [
                {
                    type: "text",
                    text: JSON.stringify(result, null, 2),
                },
            ],
        };
    }
    catch (error) {
        safeError(`[RalphX MCP] Error calling ${name}:`, error);
        safeTrace("tool.error", {
            name,
            error: error instanceof Error ? error.message : String(error),
            details: error instanceof TauriClientError ? error.details : undefined,
        });
        if (error instanceof TauriClientError) {
            return {
                content: [
                    {
                        type: "text",
                        text: formatToolErrorMessage(name, error.message, error.details),
                    },
                ],
                isError: true,
            };
        }
        return {
            content: [
                {
                    type: "text",
                    text: `ERROR: Unexpected error: ${error instanceof Error ? error.message : String(error)}`,
                },
            ],
            isError: true,
        };
    }
});
/**
 * Start the server
 */
async function main() {
    console.error("[RalphX MCP] Starting server...");
    safeError(`[RalphX MCP] Agent type: ${AGENT_TYPE}`);
    if (RALPHX_TASK_ID) {
        safeError(`[RalphX MCP] Task scope: ${RALPHX_TASK_ID}`);
    }
    if (RALPHX_PROJECT_ID) {
        safeError(`[RalphX MCP] Project scope: ${RALPHX_PROJECT_ID}`);
    }
    if (RALPHX_WORKING_DIRECTORY) {
        safeError(`[RalphX MCP] Working directory root: ${RALPHX_WORKING_DIRECTORY}`);
    }
    safeError(`[RalphX MCP] Tauri API URL: ${process.env.TAURI_API_URL || "http://127.0.0.1:3847"}`);
    safeError(`[RalphX MCP] Trace log: ${getTraceLogPath()}`);
    safeTrace("server.start", {
        argv: process.argv.slice(2),
        tauri_api_url: process.env.TAURI_API_URL || "http://127.0.0.1:3847",
    });
    // Log all tools if in debug mode or if RALPHX_DEBUG_TOOLS is set
    if (AGENT_TYPE === "debug" || process.env.RALPHX_DEBUG_TOOLS === "1") {
        logAllTools();
    }
    // Always log available tools for this agent
    const toolsByAgent = getToolsByAgent();
    const agentTools = toolsByAgent[AGENT_TYPE] || [];
    safeError(`[RalphX MCP] Tools for ${AGENT_TYPE}: ${agentTools.length > 0 ? agentTools.join(", ") : "(none - using filesystem tools)"}`);
    const transport = new StdioServerTransport();
    await server.connect(transport);
    console.error("[RalphX MCP] Server running on stdio");
    safeTrace("server.ready");
}
// Global handler for unhandled promise rejections.
// Prevents secrets in HTTP error bodies or rejected promises from leaking via Node's default stderr handler.
process.on("unhandledRejection", (reason) => {
    safeError("[RalphX MCP] Unhandled rejection:", reason);
});
main().catch((error) => {
    safeError("[RalphX MCP] Fatal error:", error);
    process.exit(1);
});
//# sourceMappingURL=index.js.map