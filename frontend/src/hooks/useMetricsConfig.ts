/**
 * useMetricsConfig - Fetch and save per-project EME calibration config
 */

import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { projectStatsApi } from "@/api/project-stats";
import { insightsStatsKeys } from "@/hooks/useInsightsMetrics";
import { projectStatsKeys } from "@/hooks/useProjectStats";
import type { MetricsConfig } from "@/types/project-stats";

// ============================================================================
// Query key factory
// ============================================================================

export const metricsConfigKeys = {
  all: ["metrics-config"] as const,
  detail: (projectId: string) =>
    [...metricsConfigKeys.all, "detail", projectId] as const,
};

// ============================================================================
// Hooks
// ============================================================================

/**
 * Fetch the EME calibration config for a project.
 *
 * @param projectId - The project to fetch config for
 * @returns TanStack Query result with MetricsConfig data
 */
export function useMetricsConfig(projectId: string | undefined) {
  return useQuery<MetricsConfig, Error>({
    queryKey: metricsConfigKeys.detail(projectId ?? ""),
    queryFn: () => projectStatsApi.getMetricsConfig(projectId!),
    enabled: !!projectId,
    staleTime: 10 * 60 * 1000,
  });
}

/**
 * Save the EME calibration config for a project.
 * On success, invalidates both the metrics config cache and the project stats
 * cache so the EME estimate recomputes with the new config.
 *
 * @param projectId - The project to save config for
 * @returns TanStack Mutation result
 */
export function useSaveMetricsConfig(projectId: string | undefined) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (config: MetricsConfig) => {
      if (!projectId) {
        throw new Error("Project metrics calibration requires a project scope");
      }
      return projectStatsApi.saveMetricsConfig(projectId, config);
    },
    onSuccess: () => {
      if (!projectId) return;
      queryClient.invalidateQueries({
        queryKey: metricsConfigKeys.detail(projectId),
      });
      queryClient.invalidateQueries({
        queryKey: projectStatsKeys.byProject(projectId),
      });
      queryClient.invalidateQueries({
        queryKey: insightsStatsKeys.all,
      });
    },
  });
}
