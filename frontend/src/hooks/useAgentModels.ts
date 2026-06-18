import { useMemo } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import {
  agentModelsApi,
  type UpsertCustomAgentModelInput,
} from "@/api/agent-models";
import {
  buildAgentModelRegistry,
  type AgentModelRegistry,
} from "@/lib/agent-models";

export const agentModelKeys = {
  all: ["agent", "models"] as const,
};

const EMPTY_MODELS: readonly [] = [];

const EMPTY_REGISTRY: AgentModelRegistry = { claude: [], codex: [] };

export function useAgentModels() {
  const queryClient = useQueryClient();
  const query = useQuery({
    queryKey: agentModelKeys.all,
    queryFn: agentModelsApi.list,
    staleTime: 1000 * 60 * 5,
    gcTime: 1000 * 60 * 10,
  });

  const models = query.data ?? EMPTY_MODELS;
  const registry = useMemo<AgentModelRegistry>(
    () => (models.length > 0 ? buildAgentModelRegistry(models) : EMPTY_REGISTRY),
    [models]
  );
  const isReady = query.isSuccess && models.length > 0;

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
    isReady,
    isLoading: query.isLoading,
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
