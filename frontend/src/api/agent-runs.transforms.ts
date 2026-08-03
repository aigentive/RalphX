import type { z } from "zod";
import { AgentRunAttributionSchema } from "./agent-runs.schemas";
import type { AgentRunAttribution } from "./agent-runs.types";

export function transformAgentRunAttribution(
  raw: z.infer<typeof AgentRunAttributionSchema>,
): AgentRunAttribution {
  return {
    id: raw.id, conversationId: raw.conversation_id, status: raw.status, startedAt: raw.started_at,
    completedAt: raw.completed_at ?? null, harness: raw.harness ?? null,
    upstreamProvider: raw.upstream_provider ?? null, providerProfile: raw.provider_profile ?? null,
    providerSessionId: raw.provider_session_id ?? null, logicalModel: raw.logical_model ?? null,
    effectiveModelId: raw.effective_model_id ?? null, logicalEffort: raw.logical_effort ?? null,
    effectiveEffort: raw.effective_effort ?? null, serviceTier: raw.service_tier ?? null,
    approvalPolicy: raw.approval_policy ?? null, sandboxMode: raw.sandbox_mode ?? null,
    inputTokens: raw.input_tokens ?? null, outputTokens: raw.output_tokens ?? null,
    cacheCreationTokens: raw.cache_creation_tokens ?? null, cacheReadTokens: raw.cache_read_tokens ?? null, estimatedUsd: raw.estimated_usd ?? null,
    runChainId: raw.run_chain_id ?? null, actionKind: raw.action_kind ?? null,
    personaSlug: raw.persona_slug ?? null, agentName: raw.agent_name ?? null,
    launchRole: raw.launch_role ?? null, runtimeSource: raw.runtime_source ?? null,
  };
}
