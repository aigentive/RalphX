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
import { type RuntimeContext } from "./runtime-context.js";
export interface RequestTeamPlanArgs {
    process: string;
    teammates: unknown[];
    team_name: string;
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
export declare function handleRequestTeamPlan(args: RequestTeamPlanArgs, contextType: string, contextId: string, leadSessionId: string | undefined, runtimeContext?: RuntimeContext): Promise<{
    content: Array<{
        type: "text";
        text: string;
    }>;
    isError?: boolean;
}>;
//# sourceMappingURL=team-plan-handler.d.ts.map