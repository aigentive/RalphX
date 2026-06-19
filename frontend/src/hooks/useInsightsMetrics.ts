import { useEffect } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  getInsightsChatUsageStats,
  getInsightsPrInsights,
  getInsightsStats,
  getInsightsTrends,
} from "@/api/metrics";
import { useEventBus } from "@/providers/EventProvider";
import type { ScopeUsageStats } from "@/api/metrics";
import type { ProjectPrInsights, ProjectStats, ProjectTrends } from "@/types/project-stats";
import type { Unsubscribe } from "@/lib/event-bus";

type InsightsProjectId = string | null | undefined;

function scopeKey(projectId: InsightsProjectId): string {
  const trimmed = projectId?.trim();
  return trimmed ? `project:${trimmed}` : "all";
}

function normalizeProjectId(projectId: InsightsProjectId): string | null {
  const trimmed = projectId?.trim();
  return trimmed ? trimmed : null;
}

function eventMatchesScope(payloadProjectId: string | undefined, projectId: InsightsProjectId) {
  const selectedProjectId = normalizeProjectId(projectId);
  return selectedProjectId == null || payloadProjectId == null || payloadProjectId === selectedProjectId;
}

export const insightsStatsKeys = {
  all: ["insightsStats"] as const,
  detail: (projectId: InsightsProjectId, weekStartDay?: number, tzOffsetMinutes?: number) =>
    [
      ...insightsStatsKeys.all,
      scopeKey(projectId),
      ...(weekStartDay !== undefined ? [weekStartDay] : []),
      ...(tzOffsetMinutes !== undefined ? [tzOffsetMinutes] : []),
    ] as const,
};

export const insightsTrendsKeys = {
  all: ["insightsTrends"] as const,
  detail: (projectId: InsightsProjectId, weekStartDay?: number, tzOffsetMinutes?: number) =>
    [
      ...insightsTrendsKeys.all,
      scopeKey(projectId),
      ...(weekStartDay !== undefined ? [weekStartDay] : []),
      ...(tzOffsetMinutes !== undefined ? [tzOffsetMinutes] : []),
    ] as const,
};

export const insightsPrInsightsKeys = {
  all: ["insightsPrInsights"] as const,
  detail: (projectId: InsightsProjectId, weekStartDay?: number, tzOffsetMinutes?: number) =>
    [
      ...insightsPrInsightsKeys.all,
      scopeKey(projectId),
      ...(weekStartDay !== undefined ? [weekStartDay] : []),
      ...(tzOffsetMinutes !== undefined ? [tzOffsetMinutes] : []),
    ] as const,
};

export const insightsChatUsageStatsKeys = {
  all: ["insights-chat-usage-stats"] as const,
  detail: (projectId: InsightsProjectId) =>
    [...insightsChatUsageStatsKeys.all, scopeKey(projectId)] as const,
};

export function useInsightsStats(
  projectId: InsightsProjectId,
  weekStartDay?: number,
  tzOffsetMinutes?: number,
) {
  const queryClient = useQueryClient();
  const bus = useEventBus();

  useEffect(() => {
    const unsubscribes: Unsubscribe[] = [];
    unsubscribes.push(
      bus.subscribe<{ project_id?: string }>("task:status_changed", (payload) => {
        if (eventMatchesScope(payload.project_id, projectId)) {
          queryClient.invalidateQueries({
            queryKey: insightsStatsKeys.detail(projectId, weekStartDay, tzOffsetMinutes),
          });
          queryClient.invalidateQueries({
            queryKey: insightsTrendsKeys.detail(projectId, weekStartDay, tzOffsetMinutes),
          });
        }
      }),
    );
    return () => {
      unsubscribes.forEach((unsubscribe) => unsubscribe());
    };
  }, [bus, projectId, queryClient, tzOffsetMinutes, weekStartDay]);

  return useQuery<ProjectStats, Error>({
    queryKey: insightsStatsKeys.detail(projectId, weekStartDay, tzOffsetMinutes),
    queryFn: () => getInsightsStats(normalizeProjectId(projectId), weekStartDay, tzOffsetMinutes),
    staleTime: 60_000,
    gcTime: 5 * 60_000,
  });
}

export function useInsightsTrends(
  projectId: InsightsProjectId,
  weekStartDay?: number,
  tzOffsetMinutes?: number,
) {
  return useQuery<ProjectTrends, Error>({
    queryKey: insightsTrendsKeys.detail(projectId, weekStartDay, tzOffsetMinutes),
    queryFn: () => getInsightsTrends(normalizeProjectId(projectId), weekStartDay, tzOffsetMinutes),
    staleTime: 5 * 60 * 1000,
  });
}

export function useInsightsPrInsights(
  projectId: InsightsProjectId,
  weekStartDay?: number,
  tzOffsetMinutes?: number,
) {
  return useQuery<ProjectPrInsights, Error>({
    queryKey: insightsPrInsightsKeys.detail(projectId, weekStartDay, tzOffsetMinutes),
    queryFn: () =>
      getInsightsPrInsights(normalizeProjectId(projectId), weekStartDay, tzOffsetMinutes),
    staleTime: 60_000,
    gcTime: 5 * 60_000,
  });
}

export function useInsightsChatUsageStats(projectId: InsightsProjectId) {
  return useQuery<ScopeUsageStats, Error>({
    queryKey: insightsChatUsageStatsKeys.detail(projectId),
    queryFn: () => getInsightsChatUsageStats(normalizeProjectId(projectId)),
    staleTime: 30_000,
    gcTime: 5 * 60_000,
  });
}
