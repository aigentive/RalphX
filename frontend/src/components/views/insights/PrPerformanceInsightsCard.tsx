import type { ReactNode } from "react";
import { GitMerge, GitPullRequest, RefreshCcw, ShieldCheck } from "lucide-react";
import type { ProjectPrInsights } from "@/types/project-stats";
import { DetailCard } from "@/components/tasks/detail-views/shared/DetailCard";

interface PrPerformanceInsightsCardProps {
  insights: ProjectPrInsights;
}

function formatInt(value: number): string {
  return new Intl.NumberFormat("en-US").format(value);
}

function formatPercent(value: number): string {
  return `${Math.round(value * 100)}%`;
}

function formatHours(value: number | null): string {
  if (value == null) return "—";
  if (value < 1) return `${Math.round(value * 60)}m`;
  return `${Math.round(value)}h`;
}

function formatWeekStart(weekStart: string): string {
  const date = new Date(`${weekStart}T00:00:00`);
  return date.toLocaleDateString("en-US", { month: "short", day: "numeric" });
}

function MiniMetric({
  icon,
  label,
  value,
  sub,
}: {
  icon: ReactNode;
  label: string;
  value: string;
  sub?: string;
}) {
  return (
    <div
      data-testid="pr-performance-metric"
      className="rounded-lg p-3 flex flex-col gap-1"
      style={{ backgroundColor: "var(--overlay-faint)" }}
    >
      <div className="flex items-center gap-2 text-[0.6875rem] text-text-muted">
        {icon}
        <span className="uppercase tracking-[0.08em]">{label}</span>
      </div>
      <div className="text-[0.9375rem] font-medium text-text-primary">{value}</div>
      {sub ? <div className="text-[0.6875rem] text-text-muted">{sub}</div> : null}
    </div>
  );
}

export function PrPerformanceInsightsCard({ insights }: PrPerformanceInsightsCardProps) {
  const { summary } = insights;
  const lastWeek =
    insights.weeklyThroughput.length > 0
      ? insights.weeklyThroughput[insights.weeklyThroughput.length - 1]
      : undefined;
  const countedOrigins = insights.origins.filter((origin) => origin.countedInTotals);
  const executionOwnedRefs = insights.origins.find(
    (origin) => origin.origin === "agent_workspace_execution_owned",
  )?.totalPrs ?? 0;
  const latestPr = insights.latestPrs[0];

  return (
    <DetailCard>
      <div className="flex flex-col gap-4">
        <div className="flex items-center gap-2">
          <GitPullRequest className="w-4 h-4" style={{ color: "var(--accent-primary)" }} />
          <div className="flex flex-col gap-0.5">
            <span className="text-sm font-medium text-text-primary">PR Performance</span>
            <span className="text-[0.75rem] text-text-muted">
              {lastWeek
                ? `${formatInt(lastWeek.opened)} opened · ${formatInt(lastWeek.merged)} merged week of ${formatWeekStart(lastWeek.weekStart)}`
                : "No PR activity yet"}
            </span>
          </div>
        </div>

        <div className="grid grid-cols-2 min-[900px]:grid-cols-4 gap-3">
          <MiniMetric
            icon={<GitPullRequest className="w-3.5 h-3.5" />}
            label="PRs"
            value={formatInt(summary.totalPrs)}
            sub={`${formatInt(summary.directWorkspacePrs)} workspace · ${formatInt(summary.taskPipelinePrs)} pipeline`}
          />
          <MiniMetric
            icon={<GitMerge className="w-3.5 h-3.5" />}
            label="Merged"
            value={formatInt(summary.mergedPrs)}
            sub={`${formatPercent(summary.terminalMergeRate)} terminal merge rate`}
          />
          <MiniMetric
            icon={<RefreshCcw className="w-3.5 h-3.5" />}
            label="Rework"
            value={formatInt(summary.needsAgentPrs)}
            sub={`${formatInt(summary.changesRequestedPrs)} changes requested`}
          />
          <MiniMetric
            icon={<ShieldCheck className="w-3.5 h-3.5" />}
            label="Conversion"
            value={formatPercent(summary.directWorkspacePrConversionRate)}
            sub={`${formatInt(summary.directWorkspacesWithPrs)} / ${formatInt(summary.directWorkspaces)} workspaces`}
          />
        </div>

        <div className="grid grid-cols-1 min-[900px]:grid-cols-3 gap-3 text-[0.75rem]">
          <div className="flex flex-col gap-1.5">
            <div className="uppercase tracking-[0.08em] text-[0.625rem] text-text-muted">
              Sources
            </div>
            {countedOrigins.map((origin) => (
              <div key={origin.origin} className="text-text-secondary">
                {origin.label}: {formatInt(origin.totalPrs)} PRs
              </div>
            ))}
            {executionOwnedRefs > 0 ? (
              <div className="text-text-muted">
                {formatInt(executionOwnedRefs)} execution-owned workspace refs deduped
              </div>
            ) : null}
          </div>

          <div className="flex flex-col gap-1.5">
            <div className="uppercase tracking-[0.08em] text-[0.625rem] text-text-muted">
              Review Loop
            </div>
            <div className="text-text-secondary">
              Requested changes: {formatInt(summary.requestedChangesEvents)}
            </div>
            <div className="text-text-secondary">
              Autofix routed: {formatInt(summary.autofixNeededEvents)}
            </div>
            <div className="text-text-secondary">
              Fixes completed: {formatInt(summary.agentFixCompletedEvents)}
            </div>
          </div>

          <div className="flex flex-col gap-1.5">
            <div className="uppercase tracking-[0.08em] text-[0.625rem] text-text-muted">
              Settlement
            </div>
            <div className="text-text-secondary">
              Workspace PR cycle: {formatHours(summary.avgWorkspacePrCycleHours)}
            </div>
            <div className="text-text-secondary">
              Pipeline PR wait: {formatHours(summary.avgPlanPrWaitHours)}
            </div>
            <div className="text-text-secondary">
              Auto-merge active: {formatInt(summary.autoMergeActiveWorkspaces)}
            </div>
          </div>
        </div>

        {latestPr ? (
          <div
            className="flex flex-wrap items-center justify-between gap-2 rounded-lg px-3 py-2 text-[0.75rem]"
            style={{ backgroundColor: "var(--overlay-faint)" }}
          >
            <span className="text-text-muted">Latest</span>
            <span className="font-medium text-text-primary">
              {latestPr.prNumber != null ? `PR #${latestPr.prNumber}` : latestPr.branchName}
            </span>
            <span className="text-text-secondary">{latestPr.status.replace(/_/g, " ")}</span>
            <span className="text-text-muted">{latestPr.label}</span>
          </div>
        ) : null}
      </div>
    </DetailCard>
  );
}
