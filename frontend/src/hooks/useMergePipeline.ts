/**
 * Hook for fetching and managing merge pipeline data
 */

import { useQuery } from "@tanstack/react-query";
import { mergePipelineApi } from "@/api/merge-pipeline";

/**
 * Query key factory for merge pipeline queries
 */
export const mergePipelineKeys = {
  all: ["merge-pipeline"] as const,
  pipeline: (projectId?: string) =>
    [...mergePipelineKeys.all, "list", projectId ?? "all"] as const,
};

/**
 * Hook to fetch the merge pipeline (active, waiting, needs attention)
 */
interface UseMergePipelineOptions {
  enabled?: boolean;
  refetchInterval?: number | false;
  refetchOnWindowFocus?: boolean;
}

export function useMergePipeline(
  projectId?: string,
  options: UseMergePipelineOptions = {}
) {
  const enabled = options.enabled ?? true;
  return useQuery({
    queryKey: mergePipelineKeys.pipeline(projectId),
    queryFn: () => mergePipelineApi.getMergePipeline(projectId),
    enabled,
    refetchInterval: enabled ? options.refetchInterval ?? 5000 : false,
    refetchOnWindowFocus: options.refetchOnWindowFocus ?? enabled,
  });
}
