import { useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import { agentRunsApi, type AgentRunAttribution } from "@/api/agent-runs";

export const MAX_ATTRIBUTION_BATCH = 100;

function uniqueSortedRunIds(runIds: readonly string[]): string[] {
  return [...new Set(runIds)].sort();
}

function chunkRunIds(runIds: readonly string[]): string[][] {
  const chunks: string[][] = [];
  for (let index = 0; index < runIds.length; index += MAX_ATTRIBUTION_BATCH) {
    chunks.push(runIds.slice(index, index + MAX_ATTRIBUTION_BATCH));
  }
  return chunks;
}

export function useRunAttributions(
  runIds: readonly string[],
  options: { enabled?: boolean } = {},
) {
  const sortedRunIds = useMemo(
    () => uniqueSortedRunIds(runIds),
    [runIds],
  );
  const queryKey = useMemo(
    () => ["agent-runs", "attribution", sortedRunIds] as const,
    [sortedRunIds],
  );

  return useQuery({
    queryKey,
    queryFn: async () => {
      const chunks = chunkRunIds(sortedRunIds);
      const attributions = await Promise.all(
        chunks.map((chunk) => agentRunsApi.getAttributions(chunk)),
      );
      return new Map<string, AgentRunAttribution>(
        attributions.flat().map((attribution) => [attribution.id, attribution]),
      );
    },
    enabled: sortedRunIds.length > 0 && (options.enabled ?? true),
    staleTime: 30_000,
  });
}
