import { typedInvokeWithTransform } from "@/lib/tauri";
import { z } from "zod";
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
  getAttributions: (runIds: string[]): Promise<AgentRunAttribution[]> =>
    typedInvokeWithTransform(
      "get_agent_run_attributions",
      { runIds },
      z.array(AgentRunAttributionSchema),
      (attributions) => attributions.map(transformAgentRunAttribution),
    ),
} as const;

export type { AgentRunAttribution, RuntimeSource } from "./agent-runs.types";
