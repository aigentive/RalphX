/**
 * InsightsView - Project analytics dashboard with effort estimation
 *
 * Design: macOS Tahoe - flat backgrounds, warm orange accent, SF Pro
 * - NO purple/blue accents
 * - NO borders or glows
 * - Two-column dashboard: metrics left, EME sticky right (>=1200px)
 */

import { useCallback, useEffect, useMemo, useState, useSyncExternalStore } from "react";
import { Calendar, Download } from "lucide-react";
import { formatMinutesHuman } from "@/lib/formatters";
import { useProjectStore } from "@/stores/projectStore";
import type { ScopeUsageStats } from "@/api/metrics";
import {
  useInsightsChatUsageStats,
  useInsightsPrInsights,
  useInsightsStats,
  useInsightsTrends,
} from "@/hooks/useInsightsMetrics";
import { DetailCard } from "@/components/tasks/detail-views/shared/DetailCard";
import type {
  ProjectPrInsights,
  ProjectStats,
  ProjectTrends,
  DeliveryWeeklyThroughputPoint,
  WeeklyDataPoint,
} from "@/types/project-stats";
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
import { AgentWorkspaceInsightsCard } from "./insights/AgentWorkspaceInsightsCard";
import { DeliveryThroughputChart } from "./insights/DeliveryThroughputChart";
import { PrPerformanceInsightsCard } from "./insights/PrPerformanceInsightsCard";
import { UsageInsightsCard } from "./insights/UsageInsightsCard";
import { ProjectDropdown } from "@/components/projects/ProjectSelector";

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

function ProjectScopeSelect({
  projects,
  value,
  onChange,
}: {
  projects: Array<{ id: string; name: string }>;
  value: string | null;
  onChange: (projectId: string | null) => void;
}) {
  return (
    <ProjectDropdown
      projects={projects}
      value={value}
      onValueChange={onChange}
      includeAllProjects
      allProjectsLabel="All projects"
      allProjectsDescription="Aggregate every project in Insights"
      align="end"
      variant="insights"
      placeholder="All projects"
      testId="insights-project-filter"
      dropdownTestId="insights-project-filter-dropdown"
      listTestId="insights-project-filter-list"
      searchTestId="insights-project-filter-search"
      allProjectsTestId="insights-project-option-all"
      showMoreTestId="insights-project-filter-show-more"
      projectOptionTestId={(project) => `insights-project-option-${project.id}`}
    />
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

function scopeSlug(scopeLabel: string): string {
  return scopeLabel
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-|-$/g, "") || "all-projects";
}

function exportJSON(stats: ProjectStats, trends: ProjectTrends, scopeLabel: string): void {
  const date = new Date().toISOString().slice(0, 10);
  const content = JSON.stringify(formatJSONExport(stats, trends), null, 2);
  downloadFile(content, `ralphx-insights-${scopeSlug(scopeLabel)}-${date}.json`, "application/json");
}

function exportCSV(trends: ProjectTrends, scopeLabel: string): void {
  const date = new Date().toISOString().slice(0, 10);
  const csv = formatCSV(trends);
  downloadFile(csv, `ralphx-insights-${scopeSlug(scopeLabel)}-${date}.csv`, "text/csv");
}

function isCurrentWeek(weekStart: string, weekStartDay: number): boolean {
  const now = new Date();
  const dayOfWeek = now.getDay(); // 0=Sunday, local time
  const diff = (dayOfWeek - weekStartDay + 7) % 7;
  const weekStartDate = new Date(now.getFullYear(), now.getMonth(), now.getDate() - diff);
  const expected = formatLocalDateKey(weekStartDate);
  return weekStart === expected;
}

function formatLocalDateKey(date: Date): string {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

function getWeekLabel(data: WeeklyDataPoint[], weekStartDay: number): string {
  if (data.length === 0) return "this week";
  const last = data[data.length - 1]!;
  return isCurrentWeek(last.weekStart, weekStartDay) ? "this week" : "latest";
}

function formatDeliveryBreakdown(point: DeliveryWeeklyThroughputPoint): string {
  const parts = [
    `${point.taskDeliveries} tasks`,
    `${point.workspaceDeliveries} workspaces`,
  ];
  if (point.mergedPrs > 0) {
    parts.push(`${point.mergedPrs} merged PRs`);
  }
  return parts.join(" / ");
}

function hasDeliveryActivity(point: DeliveryWeeklyThroughputPoint): boolean {
  return point.unifiedDeliveries > 0 || point.mergedPrs > 0;
}

function formatWeekStartLabel(weekStart: string): string {
  const date = new Date(`${weekStart}T00:00:00`);
  return date.toLocaleDateString("en-US", { month: "short", day: "numeric" });
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
  scopeLabel,
}: {
  stats: ProjectStats;
  showEme: boolean;
  projectId: string | null;
  scopeLabel: string;
}) {
  if (showEme && stats.eme) {
    return (
      <EffortEstimationPanel
        lowHours={stats.eme.lowHours}
        highHours={stats.eme.highHours}
        scopeLabel={stats.eme.scopeLabel}
        taskCount={stats.eme.taskCount}
        earliestTaskDate={stats.eme.earliestTaskDate}
        latestTaskDate={stats.eme.latestTaskDate}
        {...(projectId !== null ? { projectId } : {})}
        readOnly={projectId === null}
      />
    );
  }

  return (
    <DetailCard>
      <div className="flex flex-col gap-1">
        <p className="text-[0.8125rem] font-medium" style={{ color: "var(--text-secondary)" }}>
          Task-pipeline effort estimation unlocks after 5 completed tasks
        </p>
        <p className="text-[0.75rem]" style={{ color: "var(--text-muted)" }}>
          {stats.taskCount} of 5 task-pipeline completions available in {scopeLabel}
        </p>
      </div>
    </DetailCard>
  );
}

// ============================================================================
// Main Component
// ============================================================================

export function InsightsView() {
  const projectsById = useProjectStore((state) => state.projects);
  const [selectedProjectId, setSelectedProjectId] = useState<string | null>(null);
  const [weekStartDay, setWeekStartDay] = useWeekStartDay();
  const tzOffsetMinutes = useMemo(() => -new Date().getTimezoneOffset(), []);
  const projects = useMemo(
    () => Object.values(projectsById).sort((a, b) => a.name.localeCompare(b.name)),
    [projectsById],
  );
  const selectedProject =
    selectedProjectId !== null ? projectsById[selectedProjectId] : undefined;
  const scopeLabel = selectedProject?.name ?? "All projects";

  useEffect(() => {
    if (selectedProjectId !== null && projectsById[selectedProjectId] === undefined) {
      setSelectedProjectId(null);
    }
  }, [projectsById, selectedProjectId]);

  const statsQuery = useInsightsStats(selectedProjectId, weekStartDay, tzOffsetMinutes);
  const prInsightsQuery = useInsightsPrInsights(selectedProjectId, weekStartDay, tzOffsetMinutes);
  const usageStatsQuery = useInsightsChatUsageStats(selectedProjectId);
  const trendsQuery = useInsightsTrends(selectedProjectId, weekStartDay, tzOffsetMinutes);

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
      projectId={selectedProjectId}
      scopeLabel={scopeLabel}
      projects={projects}
      onProjectScopeChange={setSelectedProjectId}
      hasEnoughForTrends={hasEnoughForTrends}
      showEme={showEme}
      weekStartDay={weekStartDay}
      onWeekStartDayChange={setWeekStartDay}
      {...(prInsightsQuery.data !== undefined ? { prInsights: prInsightsQuery.data } : {})}
      {...(usageStatsQuery.data !== undefined ? { usageStats: usageStatsQuery.data } : {})}
    />
  );
}

function InsightsContent({
  stats,
  prInsights,
  usageStats,
  trends,
  projectId,
  scopeLabel,
  projects,
  onProjectScopeChange,
  hasEnoughForTrends,
  showEme,
  weekStartDay,
  onWeekStartDayChange,
}: {
  stats: ProjectStats;
  prInsights?: ProjectPrInsights;
  usageStats?: ScopeUsageStats;
  trends: ProjectTrends;
  projectId: string | null;
  scopeLabel: string;
  projects: Array<{ id: string; name: string }>;
  onProjectScopeChange: (projectId: string | null) => void;
  hasEnoughForTrends: boolean;
  showEme: boolean;
  weekStartDay: number;
  onWeekStartDayChange: (day: number) => void;
}) {
  const weekBoundary = weekStartDay === 0 ? "Sun–Sat" : "Mon–Sun";

  const throughputWeekLabel = useMemo(() => getWeekLabel(trends.weeklyThroughput, weekStartDay), [trends.weeklyThroughput, weekStartDay]);
  const latestDelivery =
    trends.weeklyDeliveryThroughput.length > 0
      ? trends.weeklyDeliveryThroughput[trends.weeklyDeliveryThroughput.length - 1]
      : undefined;
  const latestActiveDelivery = [...trends.weeklyDeliveryThroughput]
    .reverse()
    .find(hasDeliveryActivity);
  const displayDelivery =
    latestDelivery && isCurrentWeek(latestDelivery.weekStart, weekStartDay)
      ? hasDeliveryActivity(latestDelivery)
        ? latestDelivery
        : latestActiveDelivery ?? latestDelivery
      : latestDelivery;
  const displayDeliveryIsThisWeek = displayDelivery
    ? isCurrentWeek(displayDelivery.weekStart, weekStartDay)
    : false;
  const isThisWeek = displayDelivery
    ? displayDeliveryIsThisWeek
    : throughputWeekLabel === "this week";

  const throughputHeader = useMemo(() => {
    if (displayDelivery) {
      const label = displayDeliveryIsThisWeek
        ? "this week"
        : `week of ${formatWeekStartLabel(displayDelivery.weekStart)}`;
      return `${displayDelivery.unifiedDeliveries} ${label}`;
    }
    if (trends.weeklyThroughput.length === 0) return undefined;
    const last = trends.weeklyThroughput[trends.weeklyThroughput.length - 1]!;
    return `${last.value} ${getWeekLabel(trends.weeklyThroughput, weekStartDay)}`;
  }, [
    displayDelivery,
    displayDeliveryIsThisWeek,
    trends.weeklyThroughput,
    weekStartDay,
  ]);

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
  const showTrendCharts = hasEnoughForTrends || trends.weeklyDeliveryThroughput.length > 0;

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
              {projectId === null
                ? "All-project engineering performance and effort estimation"
                : `${scopeLabel} engineering performance and effort estimation`}
            </p>
          </div>
          <div className="flex flex-wrap items-center justify-end gap-2 shrink-0">
            <ProjectScopeSelect
              projects={projects}
              value={projectId}
              onChange={onProjectScopeChange}
            />
            <WeekStartToggle value={weekStartDay} onChange={onWeekStartDayChange} />
            <div
              className="w-px h-6 mx-1"
              style={{ backgroundColor: "var(--overlay-faint)" }}
              aria-hidden="true"
            />
            <CopyMarkdownButton stats={stats} />
            <button
              onClick={() => exportJSON(stats, trends, scopeLabel)}
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
              onClick={() => exportCSV(trends, scopeLabel)}
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
            {/* Stat cards — reordered: Throughput → Success → Cycle Time → Review */}
            <div className="grid grid-cols-2 min-[800px]:grid-cols-4 gap-3">
              <StatCard
                label={isThisWeek ? "Deliveries This Week" : "Deliveries Latest Active Week"}
                value={String(displayDelivery?.unifiedDeliveries ?? stats.tasksCompletedThisWeek)}
                sub={(() => {
                  if (displayDelivery) {
                    const breakdown = formatDeliveryBreakdown(displayDelivery);
                    return displayDeliveryIsThisWeek
                      ? breakdown
                      : `week of ${formatWeekStartLabel(displayDelivery.weekStart)} · ${breakdown}`;
                  }
                  const calWeek =
                    trends.weeklyThroughput.length > 0
                      ? trends.weeklyThroughput[trends.weeklyThroughput.length - 1]!.value
                      : stats.tasksCompletedThisWeek;
                  const rolling = stats.tasksCompletedThisWeek;
                  const parts = [`${stats.tasksCompletedToday} today`];
                  if (rolling !== calWeek) parts.push(`${rolling} last 7 days`);
                  return parts.join(" · ");
                })()}
                tooltip={isThisWeek
                  ? `Deduped delivery output this calendar week (${weekBoundary}, UTC): merged task-pipeline work plus direct agent workspace PR output.`
                  : "Current week has no delivery activity; showing the most recent active delivery week."}
              />
              <StatCard
                label="Task Pipeline Success Rate"
                value={formatPercent(stats.agentSuccessRate)}
                sub={`${stats.agentSuccessCount} / ${stats.agentTotalCount} tasks · all time`}
                tooltip="Percentage of task-pipeline runs that completed successfully (merged) vs those that failed, were cancelled, or stopped."
              />
              <StatCard
                label="Task Pipeline Time"
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
              <EmeSection
                stats={stats}
                showEme={showEme}
                projectId={projectId}
                scopeLabel={scopeLabel}
              />
            </div>

            {prInsights && (
              <>
                <SectionLabel>Agent Workspaces</SectionLabel>
                <AgentWorkspaceInsightsCard insights={prInsights} />
                <SectionLabel>Pull Requests</SectionLabel>
                <PrPerformanceInsightsCard insights={prInsights} />
              </>
            )}

            {usageStats && (
              <>
                <SectionLabel>AI Usage</SectionLabel>
                <UsageInsightsCard stats={usageStats} />
              </>
            )}

            <SectionLabel>Trends</SectionLabel>
            {/* Trend charts */}
            {!showTrendCharts ? (
              <DetailCard>
                <div className="flex flex-col gap-1">
                  <p className="text-[0.8125rem] font-medium" style={{ color: "var(--text-secondary)" }}>
                    Trend charts unlock after 10 completed tasks or agent workspace deliveries
                  </p>
                  <p className="text-[0.75rem]" style={{ color: "var(--text-muted)" }}>
                    {stats.taskCount} of 10 task-pipeline completions available in {scopeLabel}
                  </p>
                </div>
              </DetailCard>
            ) : (
              <div className="grid grid-cols-1 min-[640px]:grid-cols-2 gap-3 items-start">
                <DetailCard>
                  <DeliveryThroughputChart
                    data={trends.weeklyDeliveryThroughput}
                    {...(throughputHeader !== undefined && { currentValue: throughputHeader })}
                  />
                </DetailCard>
                <DetailCard>
                  <TrendChart
                    title="Task Pipeline Execution Time"
                    data={trends.weeklyCycleTime}
                    valueFormatter={(v) => formatMinutesHuman(v * 60)}
                    primaryLabel="Execution phases"
                    secondaryData={trends.weeklyPipelineCycleTime}
                    secondaryLabel="Full task pipeline"
                    secondaryValueFormatter={(v) => formatMinutesHuman(v * 60)}
                    {...(cycleTimeHeader !== undefined && { currentValue: cycleTimeHeader })}
                    timeWindow="Last 12 months"
                  />
                </DetailCard>
                {showSuccessRateTrend && (
                  <DetailCard>
                    <TrendChart
                      title="Task Pipeline Success Rate (%)"
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
            <EmeSection
              stats={stats}
              showEme={showEme}
              projectId={projectId}
              scopeLabel={scopeLabel}
            />
          </div>
        </div>
      </div>
    </div>
  );
}
