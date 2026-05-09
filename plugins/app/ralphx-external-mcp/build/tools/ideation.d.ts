/**
 * Ideation tool handlers — Flow 2 (Phase 4)
 *
 * Tools for starting/monitoring ideation sessions, proposals, plans, and accepted-plan follow-ups.
 * Delegates multi-step operations to composites.
 */
import type { ApiKeyContext } from "../types.js";
/**
 * v1_start_ideation — create an ideation session and spawn the orchestrator agent.
 * Delegates to startIdeation composite (POST /api/external/start_ideation).
 */
export declare function handleStartIdeation(args: Record<string, unknown>, context: ApiKeyContext): Promise<string>;
/**
 * v1_get_ideation_status — get ideation session status, agent state, and proposal count.
 * GET /api/external/ideation_status/:session_id
 */
export declare function handleGetIdeationStatus(args: Record<string, unknown>, context: ApiKeyContext): Promise<string>;
/**
 * v1_send_ideation_message — send a message to the ideation agent.
 * POST /api/external/ideation_message
 */
export declare function handleSendIdeationMessage(args: Record<string, unknown>, context: ApiKeyContext): Promise<string>;
/**
 * v1_list_proposals — list proposals in an ideation session.
 * GET /api/list_session_proposals/:session_id
 */
export declare function handleListProposals(args: Record<string, unknown>, context: ApiKeyContext): Promise<string>;
/**
 * v1_get_proposal_detail — get full proposal details including steps and acceptance criteria.
 * GET /api/proposal/:proposal_id
 */
export declare function handleGetProposalDetail(args: Record<string, unknown>, context: ApiKeyContext): Promise<string>;
/**
 * v1_get_plan — get plan artifact content for an ideation session.
 * GET /api/get_session_plan/:session_id
 */
export declare function handleGetPlan(args: Record<string, unknown>, context: ApiKeyContext): Promise<string>;
/**
 * v1_accept_plan_and_schedule — saga: apply proposals → create tasks → schedule.
 * Delegates to acceptAndSchedule composite.
 */
export declare function handleAcceptPlanAndSchedule(args: Record<string, unknown>, context: ApiKeyContext): Promise<string>;
/**
 * v1_modify_proposal — update a proposal before acceptance.
 * POST /api/update_task_proposal
 */
export declare function handleModifyProposal(args: Record<string, unknown>, context: ApiKeyContext): Promise<string>;
/**
 * v1_list_ideation_sessions — list ideation sessions for a project.
 * GET /api/external/sessions/:project_id
 */
export declare function handleListIdeationSessions(args: Record<string, unknown>, context: ApiKeyContext): Promise<string>;
/**
 * v1_get_ideation_messages — read orchestrator responses for an ideation session.
 * GET /api/external/ideation_messages/:session_id
 */
export declare function handleGetIdeationMessages(args: Record<string, unknown>, context: ApiKeyContext): Promise<string>;
/**
 * v1_get_session_tasks — get tasks created from an ideation session.
 * GET /api/external/sessions/:session_id/tasks
 */
export declare function handleGetSessionTasks(args: Record<string, unknown>, context: ApiKeyContext): Promise<string>;
/**
 * v1_append_task_to_plan — append a one-off task to an accepted active ideation plan.
 * POST /api/external/sessions/:session_id/tasks
 */
export declare function handleAppendTaskToPlan(args: Record<string, unknown>, context: ApiKeyContext): Promise<string>;
/**
 * v1_analyze_dependencies — get dependency graph for proposals in a session.
 * GET /api/analyze_dependencies/:session_id
 */
export declare function handleAnalyzeDependencies(args: Record<string, unknown>, context: ApiKeyContext): Promise<string>;
/**
 * v1_trigger_plan_verification — trigger auto-verification for a session's plan.
 * POST /api/external/trigger_verification
 */
export declare function handleTriggerPlanVerification(args: Record<string, unknown>, context: ApiKeyContext): Promise<string>;
/**
 * v1_get_plan_verification — get plan verification status for a session.
 * GET /api/external/plan_verification/:session_id
 */
export declare function handleGetPlanVerification(args: Record<string, unknown>, context: ApiKeyContext): Promise<string>;
//# sourceMappingURL=ideation.d.ts.map