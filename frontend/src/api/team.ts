/**
 * Tauri invoke wrappers for team management commands
 *
 * Provides typed API functions for team lifecycle, messaging, and status queries.
 * Uses Zod schemas for response validation following the project API pattern.
 *
 * Param naming: Tauri 2.0 auto-renames snake_case Rust params to camelCase.
 * Struct params (e.g. SendTeamMessageInput) are wrapped in the param name.
 */

import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";
import { backendApiUrl } from "@/api/backend";

// ============================================================================
// Schemas (match Rust TeamStateTracker response structs)
// ============================================================================

const TeammateCostSchema = z.object({
  input_tokens: z.number(),
  output_tokens: z.number(),
  cache_creation_tokens: z.number(),
  cache_read_tokens: z.number(),
  estimated_usd: z.number(),
});

export const TeammateStatusSchema = z.object({
  name: z.string(),
  color: z.string(),
  model: z.string(),
  role: z.string(),
  status: z.string(),
  cost: TeammateCostSchema,
  spawned_at: z.string(),
  last_activity_at: z.string(),
  conversation_id: z.string().nullable().optional(),
});

export const TeamMessageSchema = z.object({
  id: z.string(),
  sender: z.string(),
  recipient: z.string().nullable(),
  content: z.string(),
  message_type: z.string(),
  timestamp: z.string(),
});

export const TeamStatusSchema = z.object({
  name: z.string(),
  context_type: z.string(),
  context_id: z.string(),
  lead_name: z.string().nullable(),
  teammates: z.array(TeammateStatusSchema),
  phase: z.string(),
  created_at: z.string(),
  message_count: z.number(),
});

// ============================================================================
// Types
// ============================================================================

export type TeamStatusResponse = z.infer<typeof TeamStatusSchema>;
export type TeammateStatusResponse = z.infer<typeof TeammateStatusSchema>;
export type TeamMessageResponse = z.infer<typeof TeamMessageSchema>;

// ============================================================================
// History Schemas (for hydrating past team sessions)
// ============================================================================

export const TeammateSnapshotSchema = z.object({
  name: z.string(),
  color: z.string(),
  model: z.string(),
  role: z.string(),
  status: z.string(),
  cost: TeammateCostSchema,
  spawnedAt: z.string(),
  lastActivityAt: z.string(),
  conversationId: z.string().nullable().optional(),
});

export const TeamSessionHistorySchema = z.object({
  id: z.string(),
  teamName: z.string(),
  leadName: z.string().nullable(),
  contextType: z.string(),
  contextId: z.string(),
  phase: z.string(),
  createdAt: z.string(),
  disbandedAt: z.string().nullable(),
  teammates: z.array(TeammateSnapshotSchema),
});

// History messages use TeamMessageRecordResponse (camelCase, createdAt instead of timestamp)
export const TeamMessageRecordSchema = z.object({
  id: z.string(),
  sender: z.string(),
  recipient: z.string().nullable(),
  content: z.string(),
  messageType: z.string(),
  createdAt: z.string(),
});

export const TeamHistoryResponseSchema = z.object({
  session: TeamSessionHistorySchema.nullable(),
  messages: z.array(TeamMessageRecordSchema),
});

export type TeammateSnapshot = z.infer<typeof TeammateSnapshotSchema>;
export type TeamSessionHistory = z.infer<typeof TeamSessionHistorySchema>;
export type TeamMessageRecord = z.infer<typeof TeamMessageRecordSchema>;
export type TeamHistoryResponse = z.infer<typeof TeamHistoryResponseSchema>;

// ============================================================================
// API Functions
// ============================================================================

// ============================================================================
// Plan Approval (calls HTTP server directly — the approve handler has the
// spawning infrastructure that Tauri commands don't have access to)
// ============================================================================

export const ApproveTeamPlanResponseSchema = z.object({
  success: z.boolean(),
  team_name: z.string(),
  teammates_spawned: z.array(z.object({
    name: z.string(),
    role: z.string(),
    model: z.string(),
    color: z.string(),
  })),
  message: z.string(),
});

export type ApproveTeamPlanResponse = z.infer<typeof ApproveTeamPlanResponseSchema>;

export async function approveTeamPlan(
  planId: string,
  contextType: string,
  contextId: string,
): Promise<ApproveTeamPlanResponse> {
  const resp = await fetch(backendApiUrl("team/plan/approve"), {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      plan_id: planId,
      context_type: contextType,
      context_id: contextId,
    }),
  });

  if (!resp.ok) {
    const errorText = await resp.text();
    throw new Error(`Failed to approve team plan: ${errorText}`);
  }

  return ApproveTeamPlanResponseSchema.parse(await resp.json());
}

// ============================================================================
// Pending Plans
// ============================================================================

export const PendingPlanFromServerSchema = z.object({
  plan_id: z.string(),
  process: z.string(),
  teammates: z.array(z.object({
    role: z.string(),
    model: z.string(),
    tools: z.array(z.string()),
    mcp_tools: z.array(z.string()),
    prompt_summary: z.string(),
    preset: z.string().nullable().optional(),
  })),
  context_type: z.string(),
  context_id: z.string(),
  created_at_ms: z.number(),
});

export const PendingPlansResponseSchema = z.object({
  plans: z.array(PendingPlanFromServerSchema),
});

export type PendingPlanFromServer = z.infer<typeof PendingPlanFromServerSchema>;

export async function getPendingPlans(contextId: string): Promise<PendingPlanFromServer[]> {
  try {
    const resp = await fetch(backendApiUrl(`team/plan/pending/${contextId}`));
    if (!resp.ok) return [];
    const data = PendingPlansResponseSchema.parse(await resp.json());
    return data.plans;
  } catch {
    return [];
  }
}

export async function rejectTeamPlan(planId: string): Promise<void> {
  const resp = await fetch(backendApiUrl("team/plan/reject"), {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ plan_id: planId }),
  });

  if (!resp.ok) {
    const errorText = await resp.text();
    throw new Error(`Failed to reject team plan: ${errorText}`);
  }
}

// ============================================================================
// API Functions (Tauri IPC)
// ============================================================================

export async function getTeamStatus(
  teamName: string,
): Promise<TeamStatusResponse | null> {
  const result = await invoke("get_team_status", { teamName });
  return result ? TeamStatusSchema.parse(result) : null;
}

export async function sendTeamMessage(
  teamName: string,
  target: string,
  content: string,
): Promise<TeamMessageResponse> {
  const result = await invoke("send_team_message", {
    input: { teamName, target, content },
  });
  return TeamMessageSchema.parse(result);
}

export async function sendTeammateMessage(
  teamName: string,
  teammateName: string,
  content: string,
): Promise<void> {
  await invoke("send_teammate_message", {
    input: { teamName, teammateName, content },
  });
}

export async function stopTeammate(
  teamName: string,
  teammateName: string,
): Promise<void> {
  await invoke("stop_teammate", { teamName, teammateName });
}

export async function stopTeam(
  teamName: string,
): Promise<void> {
  await invoke("stop_team", { teamName });
}

export async function getTeamMessages(
  teamName: string,
  limit?: number,
): Promise<TeamMessageResponse[]> {
  return z.array(TeamMessageSchema).parse(
    await invoke("get_team_messages", {
      teamName,
      ...(limit !== undefined && { limit }),
    }),
  );
}

export async function getTeamHistory(
  contextType: string,
  contextId: string,
): Promise<TeamHistoryResponse> {
  const result = await invoke("get_team_history", {
    input: { contextType, contextId },
  });
  return TeamHistoryResponseSchema.parse(result);
}

export async function getTeammateCost(
  teamName: string,
  teammateName: string,
): Promise<{ teammate_name: string; input_tokens: number; output_tokens: number; cache_creation_tokens: number; cache_read_tokens: number; estimated_usd: number }> {
  return z.object({
    teammate_name: z.string(),
    input_tokens: z.number(),
    output_tokens: z.number(),
    cache_creation_tokens: z.number(),
    cache_read_tokens: z.number(),
    estimated_usd: z.number(),
  }).parse(
    await invoke("get_teammate_cost", { teamName, teammateName }),
  );
}

// ============================================================================
// Team Artifacts (Tauri IPC)
// ============================================================================

export const TeamArtifactSummarySchema = z.object({
  id: z.string(),
  name: z.string(),
  artifact_type: z.string(),
  version: z.number(),
  content_preview: z.string(),
  created_at: z.string(),
  author_teammate: z.string().nullable().optional(),
});

export const GetTeamArtifactsResponseSchema = z.object({
  artifacts: z.array(TeamArtifactSummarySchema),
  count: z.number(),
});

export type TeamArtifactSummary = z.infer<typeof TeamArtifactSummarySchema>;
export type GetTeamArtifactsResponse = z.infer<typeof GetTeamArtifactsResponseSchema>;

export async function getTeamArtifacts(
  sessionId: string,
): Promise<GetTeamArtifactsResponse> {
  const result = await invoke("get_team_artifacts_by_session", { sessionId });
  return GetTeamArtifactsResponseSchema.parse(result);
}
