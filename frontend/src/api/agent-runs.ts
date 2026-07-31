import { typedInvokeWithTransform } from "@/lib/tauri";
import { AgentRunAttributionSchema } from "./agent-runs.schemas";
import { transformAgentRunAttribution } from "./agent-runs.transforms";
import type { AgentRunAttribution } from "./agent-runs.types";

export const agentRunsApi = {
  getAttribution: (runId: string): Promise<AgentRunAttribution> =>
    typedInvokeWithTransform(
      "get_agent_run_attribution",
      { runId },
      AgentRunAttributionSchema,
      transformAgentRunAttribution,
    ),
} as const;

export type { AgentRunAttribution } from "./agent-runs.types";
