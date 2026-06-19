import { Clock3, GitPullRequest, PanelsTopLeft, Wrench } from "lucide-react";
import type { ProjectPrInsights, WorkspaceStateDwellTime } from "@/types/project-stats";
import { DetailCard } from "@/components/tasks/detail-views/shared/DetailCard";

interface AgentWorkspaceInsightsCardProps {
  insights: ProjectPrInsights;
}

function formatInt(value: number): string {
  return new Intl.NumberFormat("en-US").format(value);
}

function formatPercent(value: number): string {
  return `${Math.round(value * 100)}%`;
}

function formatMinutes(value: number): string {
  if (value < 60) return `${Math.round(value)}m`;
  const hours = value / 60;
  if (hours < 48) return `${Math.round(hours)}h`;
  return `${Math.round(hours / 24)}d`;
}

function WorkspaceMetric({
  label,
  value,
  sub,
}: {
  label: string;
  value: string;
  sub: string;
}) {
  return (
    <div
      className="rounded-lg p-3 flex flex-col gap-1"
      style={{ backgroundColor: "var(--overlay-faint)" }}
    >
      <span className="text-[0.6875rem] uppercase tracking-[0.08em] text-text-muted">
        {label}
      </span>
      <span className="text-[0.9375rem] font-medium text-text-primary">{value}</span>
      <span className="text-[0.6875rem] text-text-muted">{sub}</span>
    </div>
  );
}

function DwellRow({
  dwell,
  maxMinutes,
}: {
  dwell: WorkspaceStateDwellTime;
  maxMinutes: number;
}) {
  const pct = maxMinutes > 0 ? Math.max(6, Math.round((dwell.avgMinutes / maxMinutes) * 100)) : 0;
  return (
    <div className="flex items-center gap-2 text-[0.75rem]">
      <span className="w-32 truncate text-text-secondary">{dwell.label}</span>
      <div
        className="flex-1 h-1.5 rounded-full overflow-hidden"
        style={{ backgroundColor: "var(--overlay-weak)" }}
      >
        <div
          className="h-full rounded-full"
          style={{ width: `${pct}%`, backgroundColor: "var(--accent-primary)" }}
        />
      </div>
      <span className="w-12 text-right tabular-nums text-text-muted">
        {formatMinutes(dwell.avgMinutes)}
      </span>
    </div>
  );
}

export function AgentWorkspaceInsightsCard({ insights }: AgentWorkspaceInsightsCardProps) {
  const { summary } = insights;
  const directOrigin = insights.origins.find((origin) => origin.origin === "agent_workspace_direct");
  const directMerged = directOrigin?.mergedPrs ?? 0;
  const dwellTimes = insights.workspaceDwellTimes.filter((dwell) => dwell.avgMinutes > 0).slice(0, 5);
  const maxDwell = dwellTimes.reduce((max, dwell) => Math.max(max, dwell.avgMinutes), 0);

  return (
    <DetailCard>
      <div className="flex flex-col gap-4">
        <div className="flex items-center gap-2">
          <PanelsTopLeft className="w-4 h-4" style={{ color: "var(--accent-primary)" }} />
          <div className="flex flex-col gap-0.5">
            <span className="text-sm font-medium text-text-primary">Agent Workspaces</span>
            <span className="text-[0.75rem] text-text-muted">
              {formatInt(summary.directWorkspaces)} direct workspaces /{" "}
              {formatInt(summary.directWorkspacesWithPrs)} produced PRs
            </span>
          </div>
        </div>

        <div className="grid grid-cols-2 min-[900px]:grid-cols-4 gap-3">
          <WorkspaceMetric
            label="Workspaces"
            value={formatInt(summary.directWorkspaces)}
            sub={`${formatInt(summary.totalWorkspaces)} total including task-linked`}
          />
          <WorkspaceMetric
            label="PR Conversion"
            value={formatPercent(summary.directWorkspacePrConversionRate)}
            sub={`${formatInt(summary.directWorkspacesWithPrs)} direct workspace PRs`}
          />
          <WorkspaceMetric
            label="Merged"
            value={formatInt(directMerged)}
            sub={`${formatPercent(summary.terminalMergeRate)} PR terminal merge rate`}
          />
          <WorkspaceMetric
            label="Needs Agent"
            value={formatInt(summary.needsAgentPrs)}
            sub={`${formatInt(summary.agentFixCompletedEvents)} fixes completed`}
          />
        </div>

        <div className="grid grid-cols-1 min-[900px]:grid-cols-[1fr_1.2fr] gap-3">
          <div className="flex flex-col gap-1.5 text-[0.75rem]">
            <div className="flex items-center gap-1.5 uppercase tracking-[0.08em] text-[0.625rem] text-text-muted">
              <GitPullRequest className="w-3.5 h-3.5" />
              Publication
            </div>
            <div className="text-text-secondary">
              Workspace PR cycle:{" "}
              {summary.avgWorkspacePrCycleHours == null
                ? "-"
                : `${Math.round(summary.avgWorkspacePrCycleHours)}h`}
            </div>
            <div className="text-text-secondary">
              Supervision enabled: {formatInt(summary.supervisionEnabledWorkspaces)}
            </div>
            <div className="text-text-secondary">
              Auto-merge active: {formatInt(summary.autoMergeActiveWorkspaces)}
            </div>
          </div>

          <div className="flex flex-col gap-2">
            <div className="flex items-center gap-1.5 uppercase tracking-[0.08em] text-[0.625rem] text-text-muted">
              <Clock3 className="w-3.5 h-3.5" />
              Workspace State Time
            </div>
            {dwellTimes.length > 0 ? (
              <div className="flex flex-col gap-1.5">
                {dwellTimes.map((dwell) => (
                  <DwellRow
                    key={`${dwell.stateFamily}:${dwell.state}`}
                    dwell={dwell}
                    maxMinutes={maxDwell}
                  />
                ))}
              </div>
            ) : (
              <div className="flex items-center gap-1.5 text-[0.75rem] text-text-muted">
                <Wrench className="w-3.5 h-3.5" />
                No completed workspace state spans yet
              </div>
            )}
          </div>
        </div>
      </div>
    </DetailCard>
  );
}
