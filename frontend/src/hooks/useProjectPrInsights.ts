import { useQuery } from "@tanstack/react-query";
import { getProjectPrInsights } from "@/api/metrics";
import type { ProjectPrInsights } from "@/types/project-stats";

export const projectPrInsightsKeys = {
  all: ["projectPrInsights"] as const,
  detail: (projectId: string, weekStartDay?: number, tzOffsetMinutes?: number) =>
    [
      ...projectPrInsightsKeys.all,
      projectId,
      ...(weekStartDay !== undefined ? [weekStartDay] : []),
      ...(tzOffsetMinutes !== undefined ? [tzOffsetMinutes] : []),
    ] as const,
};

export function useProjectPrInsights(
  projectId: string | undefined,
  weekStartDay?: number,
  tzOffsetMinutes?: number,
) {
  return useQuery<ProjectPrInsights, Error>({
    queryKey: projectPrInsightsKeys.detail(projectId ?? "", weekStartDay, tzOffsetMinutes),
    queryFn: () => getProjectPrInsights(projectId!, weekStartDay, tzOffsetMinutes),
    enabled: !!projectId,
    staleTime: 60_000,
    gcTime: 5 * 60_000,
  });
}
