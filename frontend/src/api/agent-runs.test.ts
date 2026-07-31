import { describe, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import { agentRunsApi } from "./agent-runs";
import { AgentRunAttributionSchema } from "./agent-runs.schemas";
import { transformAgentRunAttribution } from "./agent-runs.transforms";

const fullPayload = {
  id: "run-1", conversation_id: "conversation-1", status: "completed", started_at: "2026-07-31T00:00:00Z", completed_at: "2026-07-31T00:00:42Z",
  harness: "codex", upstream_provider: "openai", provider_profile: "default", provider_session_id: "session-1",
  logical_model: "gpt-5", effective_model_id: "gpt-5.6", logical_effort: "medium", effective_effort: "high", service_tier: "fast",
  approval_policy: "never", sandbox_mode: "workspace-write", input_tokens: 11, output_tokens: 22, cache_creation_tokens: 33, cache_read_tokens: 44, estimated_usd: 0.0123,
  run_chain_id: "chain-1", action_kind: "workspace_review", persona_slug: "reviewer", agent_name: "ralphx-reviewer", launch_role: "workspace_reviewer", runtime_source: "role_default",
};

describe("agentRunsApi", () => {
  it("parses and transforms a complete snake_case attribution", () => {
    expect(transformAgentRunAttribution(AgentRunAttributionSchema.parse(fullPayload))).toMatchObject({
      conversationId: "conversation-1", effectiveModelId: "gpt-5.6", estimatedUsd: 0.0123, launchRole: "workspace_reviewer",
    });
  });

  it("accepts a minimal attribution payload and invokes with camelCase args", async () => {
    invoke.mockResolvedValue({ id: "run-1", conversation_id: "conversation-1", status: "running", started_at: "2026-07-31T00:00:00Z" });
    await expect(agentRunsApi.getAttribution("run-1")).resolves.toMatchObject({ id: "run-1", completedAt: null, agentName: null });
    expect(invoke).toHaveBeenCalledWith("get_agent_run_attribution", { runId: "run-1" });
  });
});
