/**
 * Pure formatting functions for Insights export features.
 * Extracted from InsightsView.tsx to enable unit testing without DOM dependencies.
 */

import type { ProjectStats, ProjectTrends } from "@/types/project-stats";

// ─── Constants ────────────────────────────────────────────────────────────────

export const MIN_TASKS_FOR_TRENDS = 10;
export const MIN_TASKS_FOR_EME = 5;

// ─── CSV formatting ───────────────────────────────────────────────────────────

/**
 * Format project trends data as a CSV string.
 * Merges all three weekly series by week_start into a single table.
 *
 * Headers include task throughput plus deduped delivery throughput when present.
 */
export function formatCSV(trends: ProjectTrends): string {
  const weekMap = new Map<
    string,
    {
      throughput?: number;
      unified_deliveries?: number;
      task_deliveries?: number;
      workspace_deliveries?: number;
      merged_prs?: number;
      cycle_time_hours?: number;
      success_rate_pct?: number;
    }
  >();

  for (const pt of trends.weeklyDeliveryThroughput) {
    const entry = weekMap.get(pt.weekStart) ?? {};
    entry.unified_deliveries = pt.unifiedDeliveries;
    entry.task_deliveries = pt.taskDeliveries;
    entry.workspace_deliveries = pt.workspaceDeliveries;
    entry.merged_prs = pt.mergedPrs;
    weekMap.set(pt.weekStart, entry);
  }
  for (const pt of trends.weeklyThroughput) {
    const entry = weekMap.get(pt.weekStart) ?? {};
    entry.throughput = pt.value;
    weekMap.set(pt.weekStart, entry);
  }
  for (const pt of trends.weeklyCycleTime) {
    const entry = weekMap.get(pt.weekStart) ?? {};
    entry.cycle_time_hours = +(pt.value / 60).toFixed(2);
    weekMap.set(pt.weekStart, entry);
  }
  for (const pt of trends.weeklySuccessRate) {
    const entry = weekMap.get(pt.weekStart) ?? {};
    entry.success_rate_pct = +pt.value.toFixed(1);
    weekMap.set(pt.weekStart, entry);
  }

  const rows = Array.from(weekMap.entries())
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([week, data]) =>
      [
        week,
        data.throughput ?? "",
        data.unified_deliveries ?? "",
        data.task_deliveries ?? "",
        data.workspace_deliveries ?? "",
        data.merged_prs ?? "",
        data.cycle_time_hours ?? "",
        data.success_rate_pct ?? "",
      ].join(",")
    );

  return [
    "week_start,throughput,unified_deliveries,task_deliveries,workspace_deliveries,merged_prs,cycle_time_hours,success_rate_pct",
    ...rows,
  ].join("\n");
}

// ─── JSON formatting ──────────────────────────────────────────────────────────

export interface JSONExportShape {
  stats: ProjectStats;
  trends: ProjectTrends;
  exported_at: string;
}

/**
 * Build the JSON export object for insights data.
 * Returns a plain object (not serialized) so callers can stringify as needed.
 */
export function formatJSONExport(
  stats: ProjectStats,
  trends: ProjectTrends
): JSONExportShape {
  return {
    stats,
    trends,
    exported_at: new Date().toISOString(),
  };
}

// ─── Threshold logic ──────────────────────────────────────────────────────────

/**
 * Whether trend charts should be shown (requires enough completed tasks).
 */
export function shouldShowTrends(taskCount: number): boolean {
  return taskCount >= MIN_TASKS_FOR_TRENDS;
}

/**
 * Whether the EME (Effort Model Estimate) panel should be shown.
 * Requires both enough tasks AND a non-null EME value.
 */
export function shouldShowEme(taskCount: number, hasEme: boolean): boolean {
  return hasEme && taskCount >= MIN_TASKS_FOR_EME;
}
