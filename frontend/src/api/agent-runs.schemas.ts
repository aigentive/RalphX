import { z } from "zod";

const nullableString = z.string().nullable().optional();
const nullableNumber = z.number().nullable().optional();

export const AgentRunAttributionSchema = z.object({
  id: z.string().min(1),
  conversation_id: z.string().min(1),
  status: z.string().min(1),
  started_at: z.string().min(1),
  completed_at: nullableString,
  harness: nullableString,
  upstream_provider: nullableString,
  provider_profile: nullableString,
  provider_session_id: nullableString,
  logical_model: nullableString,
  effective_model_id: nullableString,
  logical_effort: nullableString,
  effective_effort: nullableString,
  service_tier: nullableString,
  approval_policy: nullableString,
  sandbox_mode: nullableString,
  input_tokens: nullableNumber,
  output_tokens: nullableNumber,
  cache_creation_tokens: nullableNumber,
  cache_read_tokens: nullableNumber,
  estimated_usd: nullableNumber,
  run_chain_id: nullableString,
  action_kind: nullableString,
  persona_slug: nullableString,
  agent_name: nullableString,
  launch_role: nullableString,
  runtime_source: nullableString,
});
