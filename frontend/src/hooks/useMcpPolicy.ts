import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import {
  mcpPolicyApi,
  type McpServerOverrideInput,
  type McpToolOverrideInput,
  type RetryLegacyMcpRepairInput,
} from "@/api/mcp-policy";
import type { Harness } from "@/api/ideation-harness";

export const mcpPolicyKeys = {
  all: ["mcp-policy"] as const,
  catalog: (projectId: string | null, provider: Harness | null) =>
    ["mcp-policy", "catalog", { projectId, provider }] as const,
};

export function useMcpPolicy(
  projectId: string | null,
  provider: Harness | null,
  enabled: boolean,
) {
  const queryClient = useQueryClient();
  const queryKey = mcpPolicyKeys.catalog(projectId, provider);
  const query = useQuery({
    queryKey,
    queryFn: () => mcpPolicyApi.get({ projectId, provider }),
    enabled: enabled && provider !== null,
    staleTime: 15_000,
  });
  const invalidate = () => queryClient.invalidateQueries({ queryKey });

  const serverMutation = useMutation({
    mutationFn: (input: McpServerOverrideInput) =>
      input.state === "follow"
        ? mcpPolicyApi.clearServer(input)
        : mcpPolicyApi.updateServer(input),
    onSuccess: invalidate,
  });
  const toolMutation = useMutation({
    mutationFn: (input: McpToolOverrideInput) =>
      input.state === "follow"
        ? mcpPolicyApi.clearTool(input)
        : mcpPolicyApi.updateTool(input),
    onSuccess: invalidate,
  });
  const repairMutation = useMutation({
    mutationFn: (input: RetryLegacyMcpRepairInput) =>
      mcpPolicyApi.retryLegacyRepair(input),
    onSettled: invalidate,
  });

  return {
    catalog: query.data,
    isLoading: query.isLoading,
    isFetching: query.isFetching,
    error: query.error,
    refresh: query.refetch,
    refreshProvider: async (provider: Harness) => {
      await mcpPolicyApi.refresh({ projectId, provider });
      return query.refetch();
    },
    updateServer: serverMutation.mutateAsync,
    updateTool: toolMutation.mutateAsync,
    retryLegacyRepair: repairMutation.mutateAsync,
    isUpdating:
      serverMutation.isPending || toolMutation.isPending || repairMutation.isPending,
  };
}
