/**
 * Ideation tool handlers — Flow 2 (Phase 4)
 *
 * Tools for starting/monitoring ideation sessions, proposals, plans, and accepted-plan follow-ups.
 * Delegates multi-step operations to composites.
 */

import { getBackendClient, BackendError } from "../backend-client.js";
import { startIdeation } from "../composites/start-ideation.js";
import { acceptAndSchedule } from "../composites/accept-and-schedule.js";
import type { ApiKeyContext } from "../types.js";

function handleError(err: unknown): string {
  if (err instanceof BackendError) {
    return JSON.stringify(
      { error: "backend_error", status: err.statusCode, message: err.message },
      null,
      2
    );
  }
  return JSON.stringify(
    { error: "unexpected_error", message: String(err) },
    null,
    2
  );
}

/**
 * v1_start_ideation — create an ideation session and spawn the orchestrator agent.
 * Delegates to startIdeation composite (POST /api/external/start_ideation).
 */
export async function handleStartIdeation(
  args: Record<string, unknown>,
  context: ApiKeyContext
): Promise<string> {
  const projectId = args.project_id as string;
  const prompt = args.prompt as string;
  const idempotencyKey = args.idempotency_key as string | undefined;
  if (!projectId) {
    return JSON.stringify({ error: "missing_argument", message: "project_id is required" }, null, 2);
  }
  if (!prompt) {
    return JSON.stringify({ error: "missing_argument", message: "prompt is required" }, null, 2);
  }
  try {
    const result = await startIdeation({ projectId, prompt, idempotencyKey }, context);
    return JSON.stringify(result, null, 2);
  } catch (err) {
    return handleError(err);
  }
}

/**
 * v1_get_ideation_status — get ideation session status, agent state, and proposal count.
 * GET /api/external/ideation_status/:session_id
 */
export async function handleGetIdeationStatus(
  args: Record<string, unknown>,
  context: ApiKeyContext
): Promise<string> {
  const sessionId = args.session_id as string;
  if (!sessionId) {
    return JSON.stringify({ error: "missing_argument", message: "session_id is required" }, null, 2);
  }
  try {
    const response = await getBackendClient().get(
      `/api/external/ideation_status/${encodeURIComponent(sessionId)}`,
      context
    );
    return JSON.stringify(response.body, null, 2);
  } catch (err) {
    return handleError(err);
  }
}

/**
 * v1_send_ideation_message — send a message to the ideation agent.
 * POST /api/external/ideation_message
 */
export async function handleSendIdeationMessage(
  args: Record<string, unknown>,
  context: ApiKeyContext
): Promise<string> {
  const sessionId = args.session_id as string;
  const message = args.message as string;
  if (!sessionId) {
    return JSON.stringify({ error: "missing_argument", message: "session_id is required" }, null, 2);
  }
  if (!message) {
    return JSON.stringify({ error: "missing_argument", message: "message is required" }, null, 2);
  }
  try {
    const response = await getBackendClient().post(
      "/api/external/ideation_message",
      context,
      { session_id: sessionId, message }
    );
    return JSON.stringify(response.body, null, 2);
  } catch (err) {
    return handleError(err);
  }
}

/**
 * v1_list_proposals — list proposals in an ideation session.
 * GET /api/list_session_proposals/:session_id
 */
export async function handleListProposals(
  args: Record<string, unknown>,
  context: ApiKeyContext
): Promise<string> {
  const sessionId = args.session_id as string;
  if (!sessionId) {
    return JSON.stringify({ error: "missing_argument", message: "session_id is required" }, null, 2);
  }
  try {
    const response = await getBackendClient().get(
      `/api/list_session_proposals/${encodeURIComponent(sessionId)}`,
      context
    );
    return JSON.stringify(response.body, null, 2);
  } catch (err) {
    return handleError(err);
  }
}

/**
 * v1_get_proposal_detail — get full proposal details including steps and acceptance criteria.
 * GET /api/proposal/:proposal_id
 */
export async function handleGetProposalDetail(
  args: Record<string, unknown>,
  context: ApiKeyContext
): Promise<string> {
  const proposalId = args.proposal_id as string;
  if (!proposalId) {
    return JSON.stringify({ error: "missing_argument", message: "proposal_id is required" }, null, 2);
  }
  try {
    const response = await getBackendClient().get(
      `/api/proposal/${encodeURIComponent(proposalId)}`,
      context
    );
    return JSON.stringify(response.body, null, 2);
  } catch (err) {
    return handleError(err);
  }
}

/**
 * v1_get_plan — get plan artifact content for an ideation session.
 * GET /api/get_session_plan/:session_id
 */
export async function handleGetPlan(
  args: Record<string, unknown>,
  context: ApiKeyContext
): Promise<string> {
  const sessionId = args.session_id as string;
  if (!sessionId) {
    return JSON.stringify({ error: "missing_argument", message: "session_id is required" }, null, 2);
  }
  try {
    const response = await getBackendClient().get(
      `/api/get_session_plan/${encodeURIComponent(sessionId)}`,
      context
    );
    return JSON.stringify(response.body, null, 2);
  } catch (err) {
    return handleError(err);
  }
}

/**
 * v1_accept_plan_and_schedule — saga: apply proposals → create tasks → schedule.
 * Delegates to acceptAndSchedule composite.
 */
export async function handleAcceptPlanAndSchedule(
  args: Record<string, unknown>,
  context: ApiKeyContext
): Promise<string> {
  const sessionId = args.session_id as string;
  if (!sessionId) {
    return JSON.stringify({ error: "missing_argument", message: "session_id is required" }, null, 2);
  }
  try {
    const result = await acceptAndSchedule(
      {
        sessionId,
        ...(args.base_branch_override !== undefined && {
          baseBranchOverride: args.base_branch_override as string,
        }),
      },
      context
    );
    return JSON.stringify(result, null, 2);
  } catch (err) {
    return handleError(err);
  }
}

/**
 * v1_modify_proposal — update a proposal before acceptance.
 * POST /api/update_task_proposal
 */
export async function handleModifyProposal(
  args: Record<string, unknown>,
  context: ApiKeyContext
): Promise<string> {
  const proposalId = args.proposal_id as string;
  const changes = args.changes as Record<string, unknown>;
  if (!proposalId) {
    return JSON.stringify({ error: "missing_argument", message: "proposal_id is required" }, null, 2);
  }
  if (!changes || typeof changes !== "object") {
    return JSON.stringify({ error: "missing_argument", message: "changes object is required" }, null, 2);
  }
  try {
    const response = await getBackendClient().post(
      "/api/update_task_proposal",
      context,
      { proposal_id: proposalId, ...changes }
    );
    return JSON.stringify(response.body, null, 2);
  } catch (err) {
    return handleError(err);
  }
}

/**
 * v1_list_ideation_sessions — list ideation sessions for a project.
 * GET /api/external/sessions/:project_id
 */
export async function handleListIdeationSessions(
  args: Record<string, unknown>,
  context: ApiKeyContext
): Promise<string> {
  const projectId = args.project_id as string;
  if (!projectId) {
    return JSON.stringify({ error: "missing_argument", message: "project_id is required" }, null, 2);
  }
  const params = new URLSearchParams();
  if (args.status) params.set("status", args.status as string);
  if (args.limit) params.set("limit", String(args.limit));
  if (args.updated_after) params.set("updated_after", args.updated_after as string);
  const queryString = params.toString() ? `?${params.toString()}` : "";
  try {
    const response = await getBackendClient().get(
      `/api/external/sessions/${encodeURIComponent(projectId)}${queryString}`,
      context
    );
    return JSON.stringify(response.body, null, 2);
  } catch (err) {
    return handleError(err);
  }
}

/**
 * v1_get_ideation_messages — read orchestrator responses for an ideation session.
 * GET /api/external/ideation_messages/:session_id
 */
export async function handleGetIdeationMessages(
  args: Record<string, unknown>,
  context: ApiKeyContext
): Promise<string> {
  const sessionId = args.session_id as string;
  if (!sessionId) {
    return JSON.stringify({ error: "missing_argument", message: "session_id is required" }, null, 2);
  }
  const limit = args.limit !== undefined ? Number(args.limit) : 50;
  const offset = args.offset !== undefined ? Number(args.offset) : 0;
  try {
    const params = new URLSearchParams({
      limit: String(limit),
      offset: String(offset),
    });
    const response = await getBackendClient().get(
      `/api/external/ideation_messages/${encodeURIComponent(sessionId)}?${params}`,
      context
    );
    return JSON.stringify(response.body, null, 2);
  } catch (err) {
    return handleError(err);
  }
}

/**
 * v1_get_session_tasks — get tasks created from an ideation session.
 * GET /api/external/sessions/:session_id/tasks
 */
export async function handleGetSessionTasks(
  args: Record<string, unknown>,
  context: ApiKeyContext
): Promise<string> {
  const sessionId = args.session_id as string;
  if (!sessionId) {
    return JSON.stringify({ error: "missing_argument", message: "session_id is required" }, null, 2);
  }
  try {
    const qs = new URLSearchParams();
    if (args.changed_since) qs.set("changed_since", args.changed_since as string);
    const query = qs.toString() ? `?${qs.toString()}` : "";
    const response = await getBackendClient().get(
      `/api/external/sessions/${encodeURIComponent(sessionId)}/tasks${query}`,
      context
    );
    return JSON.stringify(response.body, null, 2);
  } catch (err) {
    return handleError(err);
  }
}

/**
 * v1_append_task_to_plan — append a one-off task to an accepted active ideation plan.
 * POST /api/external/sessions/:session_id/tasks
 */
export async function handleAppendTaskToPlan(
  args: Record<string, unknown>,
  context: ApiKeyContext
): Promise<string> {
  const sessionId = args.session_id as string;
  const title = args.title as string;
  if (!sessionId) {
    return JSON.stringify({ error: "missing_argument", message: "session_id is required" }, null, 2);
  }
  if (!title) {
    return JSON.stringify({ error: "missing_argument", message: "title is required" }, null, 2);
  }

  try {
    const body: Record<string, unknown> = {
      title,
    };
    if (args.description !== undefined) body.description = args.description;
    if (args.steps !== undefined) body.steps = args.steps;
    if (args.acceptance_criteria !== undefined) {
      body.acceptanceCriteria = args.acceptance_criteria;
    }
    if (args.depends_on_task_ids !== undefined) {
      body.dependsOnTaskIds = args.depends_on_task_ids;
    }
    if (args.priority !== undefined) body.priority = args.priority;
    if (args.source_conversation_id !== undefined) {
      body.sourceConversationId = args.source_conversation_id;
    }
    if (args.source_message_id !== undefined) {
      body.sourceMessageId = args.source_message_id;
    }

    const response = await getBackendClient().post(
      `/api/external/sessions/${encodeURIComponent(sessionId)}/tasks`,
      context,
      body
    );
    return JSON.stringify(response.body, null, 2);
  } catch (err) {
    return handleError(err);
  }
}

/**
 * v1_analyze_dependencies — get dependency graph for proposals in a session.
 * GET /api/analyze_dependencies/:session_id
 */
export async function handleAnalyzeDependencies(
  args: Record<string, unknown>,
  context: ApiKeyContext
): Promise<string> {
  const sessionId = args.session_id as string;
  if (!sessionId) {
    return JSON.stringify({ error: "missing_argument", message: "session_id is required" }, null, 2);
  }
  try {
    const response = await getBackendClient().get(
      `/api/analyze_dependencies/${encodeURIComponent(sessionId)}`,
      context
    );
    return JSON.stringify(response.body, null, 2);
  } catch (err) {
    return handleError(err);
  }
}

/**
 * v1_trigger_plan_verification — trigger auto-verification for a session's plan.
 * POST /api/external/trigger_verification
 */
export async function handleTriggerPlanVerification(
  args: Record<string, unknown>,
  context: ApiKeyContext
): Promise<string> {
  const sessionId = args.session_id as string;
  if (!sessionId) {
    return JSON.stringify({ error: "missing_argument", message: "session_id is required" }, null, 2);
  }
  try {
    const response = await getBackendClient().post(
      "/api/external/trigger_verification",
      context,
      { session_id: sessionId }
    );
    return JSON.stringify(response.body, null, 2);
  } catch (err) {
    return handleError(err);
  }
}

/**
 * v1_get_plan_verification — get plan verification status for a session.
 * GET /api/external/plan_verification/:session_id
 */
export async function handleGetPlanVerification(
  args: Record<string, unknown>,
  context: ApiKeyContext
): Promise<string> {
  const sessionId = args.session_id as string;
  if (!sessionId) {
    return JSON.stringify({ error: "missing_argument", message: "session_id is required" }, null, 2);
  }
  try {
    const response = await getBackendClient().get(
      `/api/external/plan_verification/${encodeURIComponent(sessionId)}`,
      context
    );
    return JSON.stringify(response.body, null, 2);
  } catch (err) {
    return handleError(err);
  }
}
