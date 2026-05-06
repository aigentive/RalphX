import { useMemo } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import {
  agentModelsApi,
  type AgentModelResponse,
  type UpsertCustomAgentModelInput,
} from "@/api/agent-models";
import {
  AGENT_MODEL_CATALOG,
  buildAgentModelRegistry,
  type AgentModelRegistry,
  type AgentProvider,
} from "@/lib/agent-models";

export const agentModelKeys = {
  all: ["agent", "models"] as const,
};

function fallbackAgentModels(): AgentModelResponse[] {
  return (["claude", "codex"] as const satisfies readonly AgentProvider[]).flatMap(
    (provider) =>
      AGENT_MODEL_CATALOG[provider].map((model) => ({
        provider,
        modelId: model.id,
        label: model.label,
        menuLabel: model.menuLabel,
        description: model.description ?? null,
        supportedEfforts: [...model.supportedEfforts],
        defaultEffort: model.defaultEffort,
        source: "built_in" as const,
        enabled: model.enabled !== false,
        createdAt: null,
        updatedAt: null,
      }))
  );
}

const FALLBACK_AGENT_MODELS = fallbackAgentModels();

export function useAgentModels() {
  const queryClient = useQueryClient();
  const query = useQuery({
    queryKey: agentModelKeys.all,
    queryFn: agentModelsApi.list,
    staleTime: 1000 * 60 * 5,
    gcTime: 1000 * 60 * 10,
    placeholderData: FALLBACK_AGENT_MODELS,
  });

  const models = query.data ?? FALLBACK_AGENT_MODELS;
  const registry = useMemo<AgentModelRegistry>(
    () => buildAgentModelRegistry(models),
    [models]
  );

  const upsertMutation = useMutation({
    mutationFn: (input: UpsertCustomAgentModelInput) =>
      agentModelsApi.upsert(input),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: agentModelKeys.all });
    },
  });

  const deleteMutation = useMutation({
    mutationFn: (input: { provider: string; modelId: string }) =>
      agentModelsApi.delete(input.provider, input.modelId),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: agentModelKeys.all });
    },
  });

  return {
    models,
    registry,
    isLoading: query.isLoading,
    isPlaceholderData: query.isPlaceholderData,
    isError: query.isError,
    error: query.error,
    upsertModel: upsertMutation.mutate,
    upsertModelAsync: upsertMutation.mutateAsync,
    isUpserting: upsertMutation.isPending,
    upsertError: upsertMutation.error,
    deleteModel: deleteMutation.mutate,
    deleteModelAsync: deleteMutation.mutateAsync,
    isDeleting: deleteMutation.isPending,
    deleteError: deleteMutation.error,
  };
}
