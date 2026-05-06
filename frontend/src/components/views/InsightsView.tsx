/**
 * InsightsView - Project analytics dashboard with effort estimation
 *
 * Design: macOS Tahoe - flat backgrounds, warm orange accent, SF Pro
 * - NO purple/blue accents
 * - NO borders or glows
 * - Two-column dashboard: metrics left, EME sticky right (>=1200px)
 */

import { useCallback, useMemo, useSyncExternalStore } from "react";
import { Calendar, Download } from "lucide-react";
import { formatMinutesHuman } from "@/lib/formatters";
import { useProjectStore, selectActiveProject } from "@/stores/projectStore";
import type { ScopeUsageStats } from "@/api/metrics";
import { useProjectStats } from "@/hooks/useProjectStats";
import { useProjectChatUsageStats } from "@/hooks/useProjectChatUsageStats";
import { useProjectTrends } from "@/hooks/useProjectTrends";
import { DetailCard } from "@/components/tasks/detail-views/shared/DetailCard";
import type { ProjectStats, ProjectTrends, WeeklyDataPoint } from "@/types/project-stats";
import {
  formatCSV,
  formatJSONExport,
  shouldShowTrends,
  shouldShowEme,
} from "@/lib/insights-export";
import { StatCard } from "./insights/StatCard";
import { TrendChart } from "./insights/TrendChart";
import { EffortEstimationPanel } from "./insights/EffortEstimationPanel";
import {
  CycleTimeBreakdown,
  ColumnDwellTimeBreakdown,
  CopyMarkdownButton,
} from "./insights/MetricsDetails";
import { UsageInsightsCard } from "./insights/UsageInsightsCard";

// ============================================================================
// Week Start Day Preference (localStorage-backed)
// ============================================================================

const WEEK_START_KEY = "ralphx:insights:weekStartDay";

function getWeekStartDay(): number {
  const stored = localStorage.getItem(WEEK_START_KEY);
  if (stored === "1") return 1;
  return 0; // default Sunday
}

const weekStartListeners = new Set<() => void>();
function subscribeWeekStart(cb: () => void) {
  weekStartListeners.add(cb);
  return () => { weekStartListeners.delete(cb); };
}

function useWeekStartDay(): [number, (day: number) => void] {
  const value = useSyncExternalStore(subscribeWeekStart, getWeekStartDay);
  const setValue = useCallback((day: number) => {
    localStorage.setItem(WEEK_START_KEY, String(day));
    weekStartListeners.forEach((cb) => cb());
  }, []);
  return [value, setValue];
}

function WeekStartToggle({
  value,
  onChange,
}: {
  value: number;
  onChange: (day: number) => void;
}) {
  return (
    <div
      className="flex items-center gap-1.5 rounded-lg px-2 py-1.5"
      style={{
        backgroundColor: "var(--bg-surface)",
        border: "1px solid var(--overlay-faint)",
      }}
    >
      <Calendar size={13} style={{ color: "var(--text-muted)" }} />
      {[
        { day: 0, label: "Sun" },
        { day: 1, label: "Mon" },
      ].map(({ day, label }) => (
        <button
          key={day}
          onClick={() => onChange(day)}
          className="rounded px-2 py-0.5 text-[0.6875rem] font-medium transition-colors"
          style={
            value === day
              ? { backgroundColor: "var(--accent-primary)", color: "var(--text-on-accent)" }
              : { color: "var(--text-muted)" }
          }
          title={`Week starts on ${day === 0 ? "Sunday" : "Monday"}`}
        >
          {label}
        </button>
      ))}
    </div>
  );
}

// ============================================================================
// Helpers
// ============================================================================

function formatPercent(value: number): string {
  return `${Math.round(value * 100)}%`;
}

function downloadFile(content: string, filename: string, mimeType: string): void {
  const blob = new Blob([content], { type: mimeType });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  a.click();
  URL.revokeObjectURL(url);
}

function exportJSON(stats: ProjectStats, trends: ProjectTrends): void {
  const date = new Date().toISOString().slice(0, 10);
  const content = JSON.stringify(formatJSONExport(stats, trends), null, 2);
  downloadFile(content, `ralphx-insights-${date}.json`, "application/json");
}

function exportCSV(trends: ProjectTrends): void {
  const date = new Date().toISOString().slice(0, 10);
  const csv = formatCSV(trends);
  downloadFile(csv, `ralphx-insights-${date}.csv`, "text/csv");
}

function isCurrentWeek(weekStart: string, weekStartDay: number): boolean {
  const now = new Date();
  const dayOfWeek = now.getDay(); // 0=Sunday, local time
  const diff = (dayOfWeek - weekStartDay + 7) % 7;
  const weekStartDate = new Date(now);
  weekStartDate.setDate(now.getDate() - diff);
  const expected = weekStartDate.toISOString().slice(0, 10);
  return weekStart === expected;
}

function getWeekLabel(data: WeeklyDataPoint[], weekStartDay: number): string {
  if (data.length === 0) return "this week";
  const last = data[data.length - 1]!;
  return isCurrentWeek(last.weekStart, weekStartDay) ? "this week" : "latest";
}

function getAvgPipelineTimeDisplay(stats: ProjectStats): string {
  if (stats.avgPipelineMinutes == null) return "—";
  return formatMinutesHuman(stats.avgPipelineMinutes);
}

function SectionLabel({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex items-center gap-3 mt-2">
      <span
        className="text-[0.625rem] font-semibold uppercase tracking-[0.12em]"
        style={{ color: "var(--text-muted)" }}
      >
        {children}
      </span>
      <div className="flex-1 h-px" style={{ backgroundColor: "var(--overlay-faint)" }} />
    </div>
  );
}

// ============================================================================
// EME Panel (right column / inline depending on breakpoint)
// ============================================================================

function EmeSection({
  stats,
  showEme,
  projectId,
}: {
  stats: ProjectStats;
  showEme: boolean;
  projectId: string;
}) {
  if (showEme && stats.eme) {
    return (
      <EffortEstimationPanel
        lowHours={stats.eme.lowHours}
        highHours={stats.eme.highHours}
        taskCount={stats.eme.taskCount}
        earliestTaskDate={stats.eme.earliestTaskDate}
        latestTaskDate={stats.eme.latestTaskDate}
        projectId={projectId}
      />
    );
  }

  return (
    <DetailCard>
      <div className="flex flex-col gap-1">
        <p className="text-[0.8125rem] font-medium" style={{ color: "var(--text-secondary)" }}>
          Effort estimation unlocks after 5 completed tasks
        </p>
        <p className="text-[0.75rem]" style={{ color: "var(--text-muted)" }}>
          {stats.taskCount} of 5 tasks completed — keep going!
        </p>
      </div>
    </DetailCard>
  );
}

// ============================================================================
// Main Component
// ============================================================================

export function InsightsView() {
  const project = useProjectStore(selectActiveProject);
  const projectId = project?.id;
  const [weekStartDay, setWeekStartDay] = useWeekStartDay();
  const tzOffsetMinutes = useMemo(() => -new Date().getTimezoneOffset(), []);

  const statsQuery = useProjectStats(projectId, weekStartDay, tzOffsetMinutes);
  const usageStatsQuery = useProjectChatUsageStats(projectId);
  const trendsQuery = useProjectTrends(projectId, weekStartDay, tzOffsetMinutes);

  // No active project
  if (!projectId) {
    return (
      <div className="flex flex-1 items-center justify-center" style={{ color: "var(--text-muted)" }}>
        <p className="text-[0.875rem]">Select a project to view insights</p>
      </div>
    );
  }

  // Loading
  if (statsQuery.isLoading || trendsQuery.isLoading) {
    return (
      <div className="flex flex-1 items-center justify-center" style={{ color: "var(--text-muted)" }}>
        <p className="text-[0.875rem]">Loading insights...</p>
      </div>
    );
  }

  // Error
  if (statsQuery.error ?? trendsQuery.error) {
    return (
      <div className="flex flex-1 items-center justify-center" style={{ color: "var(--text-muted)" }}>
        <p className="text-[0.875rem]">Failed to load insights. Try again.</p>
      </div>
    );
  }

  const stats = statsQuery.data;
  const trends = trendsQuery.data;

  if (!stats || !trends) {
    return null;
  }

  const hasEnoughForTrends = shouldShowTrends(stats.taskCount);
  const showEme = shouldShowEme(stats.taskCount, stats.eme !== null);

  return (
    <InsightsContent
      stats={stats}
      trends={trends}
      projectId={projectId}
      hasEnoughForTrends={hasEnoughForTrends}
      showEme={showEme}
      weekStartDay={weekStartDay}
      onWeekStartDayChange={setWeekStartDay}
      {...(usageStatsQuery.data !== undefined ? { usageStats: usageStatsQuery.data } : {})}
    />
  );
}

function InsightsContent({
  stats,
  usageStats,
  trends,
  projectId,
  hasEnoughForTrends,
  showEme,
  weekStartDay,
  onWeekStartDayChange,
}: {
  stats: ProjectStats;
  usageStats?: ScopeUsageStats;
  trends: ProjectTrends;
  projectId: string;
  hasEnoughForTrends: boolean;
  showEme: boolean;
  weekStartDay: number;
  onWeekStartDayChange: (day: number) => void;
}) {
  const weekBoundary = weekStartDay === 0 ? "Sun–Sat" : "Mon–Sun";

  const throughputWeekLabel = useMemo(() => getWeekLabel(trends.weeklyThroughput, weekStartDay), [trends.weeklyThroughput, weekStartDay]);
  const isThisWeek = throughputWeekLabel === "this week";

  const throughputHeader = useMemo(() => {
    if (trends.weeklyThroughput.length === 0) return undefined;
    const last = trends.weeklyThroughput[trends.weeklyThroughput.length - 1]!;
    return `${last.value} ${getWeekLabel(trends.weeklyThroughput, weekStartDay)}`;
  }, [trends.weeklyThroughput, weekStartDay]);

  const cycleTimeHeader = useMemo(() => {
    if (trends.weeklyCycleTime.length === 0) return undefined;
    const last = trends.weeklyCycleTime[trends.weeklyCycleTime.length - 1]!;
    return `${formatMinutesHuman(last.value * 60)} ${getWeekLabel(trends.weeklyCycleTime, weekStartDay)}`;
  }, [trends.weeklyCycleTime, weekStartDay]);

  const successRateHeader = useMemo(() => {
    if (trends.weeklySuccessRate.length === 0) return undefined;
    const last = trends.weeklySuccessRate[trends.weeklySuccessRate.length - 1]!;
    return `${Math.round(last.value * 100)}% ${getWeekLabel(trends.weeklySuccessRate, weekStartDay)}`;
  }, [trends.weeklySuccessRate, weekStartDay]);

  const showSuccessRateTrend = useMemo(() => {
    if (!hasEnoughForTrends) return false;
    const rates = trends.weeklySuccessRate.map((d) => d.value);
    if (rates.length === 0) return false;
    const avg = rates.reduce((a, b) => a + b, 0) / rates.length;
    if (avg < 0.95) return true;
    const variance = rates.reduce((sum, r) => sum + (r - avg) ** 2, 0) / rates.length;
    return Math.sqrt(variance) > 0.03;
  }, [hasEnoughForTrends, trends.weeklySuccessRate]);

  return (
    <div
      data-testid="insights-view"
      className="flex flex-col flex-1 overflow-auto"
      style={{ backgroundColor: "var(--bg-base)" }}
    >
      <div className="flex flex-col gap-6 p-6 max-w-[1400px] w-full mx-auto">
        {/* Header with export buttons */}
        <div className="flex items-start justify-between gap-4">
          <div data-testid="insights-header" className="flex flex-col gap-1">
            <h1
              className="text-[1.375rem] font-semibold"
              style={{ fontFamily: "system-ui", color: "var(--text-primary)", letterSpacing: "-0.01em" }}
            >
              Insights
            </h1>
            <p className="text-[0.8125rem]" style={{ color: "var(--text-secondary)" }}>
              Project analytics and effort estimation
            </p>
          </div>
          <div className="flex items-center gap-2 shrink-0">
            <WeekStartToggle value={weekStartDay} onChange={onWeekStartDayChange} />
            <div
              className="w-px h-6 mx-1"
              style={{ backgroundColor: "var(--overlay-faint)" }}
              aria-hidden="true"
            />
            <CopyMarkdownButton stats={stats} />
            <button
              onClick={() => exportJSON(stats, trends)}
              className="flex items-center gap-2 rounded-lg px-3 py-2 text-[0.75rem] font-medium transition-colors"
              style={{
                backgroundColor: "var(--bg-surface)",
                color: "var(--text-secondary)",
                border: "1px solid var(--overlay-faint)",
              }}
              title="Download JSON"
            >
              <Download size={13} />
              <span className="hidden min-[800px]:inline">JSON</span>
            </button>
            <button
              onClick={() => exportCSV(trends)}
              className="flex items-center gap-2 rounded-lg px-3 py-2 text-[0.75rem] font-medium transition-colors"
              style={{
                backgroundColor: "var(--bg-surface)",
                color: "var(--text-secondary)",
                border: "1px solid var(--overlay-faint)",
              }}
              title="Download CSV"
            >
              <Download size={13} />
              <span className="hidden min-[800px]:inline">CSV</span>
            </button>
          </div>
        </div>

        {/* Two-column dashboard: metrics left, EME sticky right at >=1200px */}
        <div className="grid grid-cols-1 min-[1200px]:grid-cols-[1fr_320px] gap-6">
          {/* Left column: all metrics */}
          <div className="flex flex-col gap-5">
            <SectionLabel>Overview</SectionLabel>
            {/* Stat cards — reordered: Tasks → Success → Cycle Time → Review */}
            <div className="grid grid-cols-2 min-[800px]:grid-cols-4 gap-3">
              <StatCard
                label={isThisWeek ? "Tasks This Week" : "Tasks (latest week)"}
                value={String(
                  trends.weeklyThroughput.length > 0
                    ? trends.weeklyThroughput[trends.weeklyThroughput.length - 1]!.value
                    : stats.tasksCompletedThisWeek
                )}
                sub={(() => {
                  const calWeek = trends.weeklyThroughput.length > 0
                    ? trends.weeklyThroughput[trends.weeklyThroughput.length - 1]!.value
                    : stats.tasksCompletedThisWeek;
                  const rolling = stats.tasksCompletedThisWeek;
                  const parts = [`${stats.tasksCompletedToday} today`];
                  if (rolling !== calWeek) parts.push(`${rolling} last 7 days`);
                  return parts.join(" · ");
                })()}
                tooltip={isThisWeek
                  ? `Tasks merged this calendar week (${weekBoundary}, UTC). The 'last 7 days' count uses a rolling window and may differ.`
                  : "Tasks merged in the most recent week with data. No tasks merged in the current calendar week yet."}
              />
              <StatCard
                label="Agent Success Rate"
                value={formatPercent(stats.agentSuccessRate)}
                sub={`${stats.agentSuccessCount} / ${stats.agentTotalCount} tasks · all time`}
                tooltip="Percentage of tasks that completed successfully (merged) vs those that failed, were cancelled, or stopped. Higher = more reliable AI execution."
              />
              <StatCard
                label="Avg Pipeline Time"
                value={getAvgPipelineTimeDisplay(stats)}
                sub="start to merge · last 90 days"
                tooltip="Average wall-clock time a task takes from entering the pipeline to merge completion. Includes queue time, AI execution, review, and merge stages. Lower is better — most time is typically spent waiting (queue/escalation), not in active execution."
              />
              <StatCard
                label="Review Pass Rate"
                value={formatPercent(stats.reviewPassRate)}
                sub={`${stats.reviewPassCount} / ${stats.reviewTotalCount} reviews · all time`}
                tooltip="Percentage of AI code reviews that passed on first attempt without requesting changes. Higher = better first-draft quality."
              />
            </div>

            {/* EME panel — medium breakpoint (800-1199px): inline between stats and charts */}
            <div className="block min-[1200px]:hidden">
              <EmeSection stats={stats} showEme={showEme} projectId={projectId} />
            </div>

            {usageStats && (
              <>
                <SectionLabel>AI Usage</SectionLabel>
                <UsageInsightsCard stats={usageStats} />
              </>
            )}

            <SectionLabel>Trends</SectionLabel>
            {/* Trend charts */}
            {!hasEnoughForTrends ? (
              <DetailCard>
                <div className="flex flex-col gap-1">
                  <p className="text-[0.8125rem] font-medium" style={{ color: "var(--text-secondary)" }}>
                    Trend charts unlock after 10 completed tasks
                  </p>
                  <p className="text-[0.75rem]" style={{ color: "var(--text-muted)" }}>
                    {stats.taskCount} of 10 tasks completed
                  </p>
                </div>
              </DetailCard>
            ) : (
              <div className="grid grid-cols-1 min-[640px]:grid-cols-2 gap-3 items-start">
                <DetailCard>
                  <TrendChart
                    title="Weekly Throughput (tasks)"
                    data={trends.weeklyThroughput}
                    {...(throughputHeader !== undefined && { currentValue: throughputHeader })}
                    timeWindow="Last 12 months"
                  />
                </DetailCard>
                <DetailCard>
                  <TrendChart
                    title="Execution Time"
                    data={trends.weeklyCycleTime}
                    valueFormatter={(v) => formatMinutesHuman(v * 60)}
                    primaryLabel="AI execution"
                    secondaryData={trends.weeklyPipelineCycleTime}
                    secondaryLabel="Pipeline (start → merge)"
                    secondaryValueFormatter={(v) => formatMinutesHuman(v * 60)}
                    {...(cycleTimeHeader !== undefined && { currentValue: cycleTimeHeader })}
                    timeWindow="Last 12 months"
                  />
                </DetailCard>
                {showSuccessRateTrend && (
                  <DetailCard>
                    <TrendChart
                      title="Agent Success Rate (%)"
                      data={trends.weeklySuccessRate}
                      valueFormatter={(v) => `${Math.round(v * 100)}%`}
                      color="var(--status-success)"
                      {...(successRateHeader !== undefined && { currentValue: successRateHeader })}
                      timeWindow="Last 12 months"
                    />
                  </DetailCard>
                )}
              </div>
            )}

            <SectionLabel>Breakdowns</SectionLabel>
            {/* Breakdowns — full-width stack (each breakdown is a row list, not split) */}
            <div className="flex flex-col gap-3">
              <CycleTimeBreakdown phases={stats.cycleTimeBreakdown} />
              <ColumnDwellTimeBreakdown dwellTimes={stats.columnDwellTimes} />
            </div>
          </div>

          {/* Right column: EME sticky (only visible at >=1200px) */}
          <div className="hidden min-[1200px]:block min-[1200px]:sticky min-[1200px]:top-6 min-[1200px]:self-start min-[1200px]:max-h-[calc(100vh-48px)] min-[1200px]:overflow-y-auto">
            <EmeSection stats={stats} showEme={showEme} projectId={projectId} />
          </div>
        </div>
      </div>
    </div>
  );
}
