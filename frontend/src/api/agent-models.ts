import { z } from "zod";

import { typedInvoke } from "@/lib/tauri";

export const AgentModelResponseSchema = z.object({
  provider: z.string().min(1),
  modelId: z.string().min(1),
  label: z.string().min(1),
  menuLabel: z.string().min(1),
  description: z.string().nullable().optional(),
  supportedEfforts: z.array(z.string().min(1)),
  defaultEffort: z.string().min(1),
  source: z.enum(["built_in", "custom"]),
  enabled: z.boolean(),
  createdAt: z.string().nullable().optional(),
  updatedAt: z.string().nullable().optional(),
});

export type AgentModelResponse = z.infer<typeof AgentModelResponseSchema>;

export interface UpsertCustomAgentModelInput {
  provider: string;
  modelId: string;
  label: string;
  menuLabel?: string | null;
  description?: string | null;
  supportedEfforts: string[];
  defaultEffort: string;
  enabled: boolean;
}

export const agentModelsApi = {
  list(): Promise<AgentModelResponse[]> {
    return typedInvoke(
      "list_agent_models",
      {},
      z.array(AgentModelResponseSchema),
    );
  },

  upsert(input: UpsertCustomAgentModelInput): Promise<AgentModelResponse> {
    return typedInvoke(
      "upsert_custom_agent_model",
      { input },
      AgentModelResponseSchema,
    );
  },

  delete(provider: string, modelId: string): Promise<boolean> {
    return typedInvoke(
      "delete_custom_agent_model",
      { provider, modelId },
      z.boolean(),
    );
  },
} as const;
