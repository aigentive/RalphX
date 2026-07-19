/**
 * Handler for request_team_plan MCP tool
 *
 * Two-phase flow mirroring question-handler.ts:
 * 1. POST /api/team/plan/request — validates team, stores plan, returns plan_id immediately
 * 2. GET /api/team/plan/await/:plan_id — long-polls for user approval (15 min timeout)
 * 3. Returns approval result to agent as tool result
 *
 * Timeout staggering: backend timeout = 840s (14 min), client AbortController = 900,000ms (15 min).
 * Backend always fires first, returning a structured 408 response.
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { safeError } from "./redact.js";
import { buildTauriApiUrl } from "./tauri-client.js";
import { buildRuntimeTransportHeaders, } from "./runtime-context.js";
/** Timeout for long-polling (15 minutes — staggered 1 min above backend's 14 min) */
const TEAM_PLAN_TIMEOUT_MS = 15 * 60 * 1000;
const SAFE_TEAM_NAME = /^[A-Za-z0-9._-]{1,128}$/;
const SAFE_SESSION_ID = /^[A-Za-z0-9][A-Za-z0-9:_-]{0,127}$/;
function resolveTeamConfigPath(teamName) {
    if (!SAFE_TEAM_NAME.test(teamName)) {
        return null;
    }
    return path.join(os.homedir(), ".claude", "teams", teamName, "config.json");
}
function sanitizeLeadSessionId(value) {
    if (typeof value !== "string") {
        return undefined;
    }
    const trimmed = value.trim();
    if (!SAFE_SESSION_ID.test(trimmed)) {
        return undefined;
    }
    return trimmed;
}
/**
 * Handle a request_team_plan tool call.
 *
 * Flow:
 * 1. Validate team_name and team registry existence
 * 2. Resolve lead_session_id from env or team config
 * 3. POST to /api/team/plan/request — registers plan, backend emits Tauri event
 * 4. GET /api/team/plan/await/:plan_id — blocks until user approves/rejects (15 min timeout)
 * 5. Return approval result to agent
 */
export async function handleRequestTeamPlan(args, contextType, contextId, leadSessionId, runtimeContext = {}) {
    const teamName = args.team_name;
    // Validate team_name is present
    if (!teamName) {
        return {
            content: [{
                    type: "text",
                    text: `ERROR: team_name is required for request_team_plan. Pass the exact team name from your TeamCreate call.`,
                }],
            isError: true,
        };
    }
    // Validate team exists in Claude Code's registry
    const configPath = resolveTeamConfigPath(teamName);
    if (!configPath) {
        return {
            content: [{
                    type: "text",
                    text: `ERROR: Team name '${teamName}' contains unsupported characters.`,
                }],
            isError: true,
        };
    }
    if (!fs.existsSync(configPath)) {
        return {
            content: [{
                    type: "text",
                    text: `ERROR: Team '${teamName}' not found in Claude Code registry at ${configPath}. Make sure you call TeamCreate with this exact team name before calling request_team_plan.`,
                }],
            isError: true,
        };
    }
    // Resolve lead_session_id: env var first, then team config fallback
    let resolvedLeadSessionId = leadSessionId;
    if (!resolvedLeadSessionId) {
        try {
            const configContent = JSON.parse(fs.readFileSync(configPath, "utf-8"));
            const configLeadSessionId = sanitizeLeadSessionId(configContent.leadSessionId);
            if (configLeadSessionId) {
                resolvedLeadSessionId = configLeadSessionId;
                safeError(`[RalphX MCP] lead_session_id resolved from team config: ${resolvedLeadSessionId}`);
            }
            else if (configContent.leadSessionId !== undefined) {
                safeError("[RalphX MCP] Ignoring invalid lead_session_id in team config");
            }
        }
        catch (e) {
            safeError(`[RalphX MCP] Warning: could not read team config for lead_session_id fallback: ${e}`);
        }
    }
    safeError(`[RalphX MCP] request_team_plan: lead_session_id=${resolvedLeadSessionId ?? "NULL"}, env_var=${leadSessionId ?? "NOT_SET"}, team=${teamName}, context_id=${contextId || "EMPTY"}`);
    // Phase 1: Register plan with Tauri backend
    let plan_id;
    try {
        const registerResponse = await fetch(buildTauriApiUrl("team/plan/request"), {
            method: "POST",
            headers: {
                "Content-Type": "application/json",
                ...(buildRuntimeTransportHeaders(runtimeContext) ?? {}),
            },
            body: JSON.stringify({
                context_type: contextType || "ideation",
                context_id: contextId || "",
                process: args.process,
                teammates: args.teammates,
                team_name: teamName,
                lead_session_id: resolvedLeadSessionId ?? null,
            }),
        });
        if (!registerResponse.ok) {
            const errorText = await registerResponse.text().catch(() => registerResponse.statusText);
            throw new Error(`Failed to register team plan: ${errorText}`);
        }
        const result = (await registerResponse.json());
        plan_id = result.plan_id;
        safeError(`[RalphX MCP] Team plan registered: ${plan_id}`);
        // Short-circuit: auto-approved plans don't need Phase 2
        if (result.auto_approved) {
            safeError(`[RalphX MCP] Team plan ${plan_id} auto-approved — skipping Phase 2`);
            return {
                content: [{
                        type: "text",
                        text: JSON.stringify({
                            success: true,
                            plan_id: result.plan_id,
                            team_name: args.team_name,
                            teammates_spawned: result.teammates_spawned ?? [],
                            message: result.message,
                        }),
                    }],
            };
        }
    }
    catch (error) {
        safeError(`[RalphX MCP] Failed to register team plan:`, error);
        return {
            content: [
                {
                    type: "text",
                    text: JSON.stringify({
                        error: true,
                        message: `Failed to register team plan: ${error instanceof Error ? error.message : String(error)}`,
                    }),
                },
            ],
            isError: true,
        };
    }
    // Phase 2: Long-poll for user approval (15 min timeout)
    const controller = new AbortController();
    const timeoutId = setTimeout(() => controller.abort(), TEAM_PLAN_TIMEOUT_MS);
    try {
        const awaitResponse = await fetch(buildTauriApiUrl(`team/plan/await/${encodeURIComponent(plan_id)}`), {
            method: "GET",
            signal: controller.signal,
        });
        clearTimeout(timeoutId);
        if (!awaitResponse.ok) {
            if (awaitResponse.status === 408) {
                // Timeout from backend — structured response, not an error
                safeError(`[RalphX MCP] Team plan ${plan_id} timed out (backend)`);
                return {
                    content: [
                        {
                            type: "text",
                            text: JSON.stringify({
                                success: false,
                                reason: "timeout",
                                plan_id,
                                message: "Team plan approval timed out after 14 minutes. The user may be away. You can continue without approval or retry later.",
                            }),
                        },
                    ],
                };
            }
            if (awaitResponse.status === 404) {
                // Channel already removed — likely auto-approved but short-circuit didn't fire
                safeError(`[RalphX MCP] Phase 2 returned 404 for plan ${plan_id} — channel already processed`);
                return {
                    content: [
                        {
                            type: "text",
                            text: JSON.stringify({
                                success: false,
                                plan_id,
                                team_name: args.team_name,
                                teammates_spawned: [],
                                message: "Plan channel already processed (likely auto-approved). Check agent logs.",
                            }),
                        },
                    ],
                };
            }
            const errorText = await awaitResponse.text().catch(() => awaitResponse.statusText);
            throw new Error(`Team plan await error: ${errorText}`);
        }
        const approvalResult = await awaitResponse.json();
        safeError(`[RalphX MCP] Team plan ${plan_id} resolved`);
        return {
            content: [
                {
                    type: "text",
                    text: JSON.stringify(approvalResult),
                },
            ],
        };
    }
    catch (error) {
        clearTimeout(timeoutId);
        if (error instanceof Error && error.name === "AbortError") {
            // Client-side timeout (safety net — backend should fire first)
            safeError(`[RalphX MCP] Team plan ${plan_id} timed out (client)`);
            return {
                content: [
                    {
                        type: "text",
                        text: JSON.stringify({
                            success: false,
                            reason: "timeout",
                            plan_id,
                            message: "Team plan approval timed out after 15 minutes. The user may be away. You can continue without approval or retry later.",
                        }),
                    },
                ],
            };
        }
        safeError(`[RalphX MCP] Team plan await error:`, error);
        throw error;
    }
}
//# sourceMappingURL=team-plan-handler.js.map