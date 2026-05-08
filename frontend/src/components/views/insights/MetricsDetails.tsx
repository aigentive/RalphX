/**
 * Shared detail components for metrics display.
 * Used by both InsightsView and ProjectStatsCard.
 */

import { useState } from "react";
import { Copy, Check, Clock, Columns3, GitMerge, HelpCircle } from "lucide-react";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { DetailCard } from "@/components/tasks/detail-views/shared/DetailCard";
import { formatMinutesHuman } from "@/lib/formatters";
import type { ColumnDwellTime, CycleTimePhase, ProjectStats } from "@/types/project-stats";

// ============================================================================
// RateBar
// ============================================================================

interface RateBarProps {
  label: string;
  rate: number;
  passCount: number;
  totalCount: number;
  color: string;
}

function RateBar({ label, rate, passCount, totalCount, color }: RateBarProps) {
  const pct = Math.round(rate * 100);
  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex items-center justify-between">
        <span className="text-[0.75rem] text-text-secondary">
          {label}
        </span>
        <span className="text-[0.75rem] font-medium text-text-secondary">
          {pct}%
          <span className="ml-1 text-text-muted">
            ({passCount}/{totalCount})
          </span>
        </span>
      </div>
      <div
        className="h-1.5 w-full rounded-full"
        style={{ backgroundColor: "var(--overlay-weak)" }}
      >
        <div
          className="h-full rounded-full transition-all duration-300"
          style={{ width: `${pct}%`, backgroundColor: color }}
        />
      </div>
    </div>
  );
}

// ============================================================================
// CycleTimeBar
// ============================================================================

const PHASE_TOOLTIPS: Record<string, string> = {
  merged: "Total time from first transition to merge completion",
  blocked: "Time tasks spent waiting on dependencies",
  cancelled: "Time before task was cancelled",
  escalated: "Time before AI escalated to human review",
  stopped: "Time before task was manually stopped",
  failed: "Time before execution failure",
  paused: "Time spent in paused state",
  merge_conflict: "Time resolving merge conflicts",
  executing: "Active AI execution time",
  re_executing: "Re-execution time after revision",
  merging: "Time in automated merge process",
  merge_incomplete: "Time in incomplete merge state",
  reviewing: "Active AI review time",
  ready: "Time waiting in ready queue",
  pending_merge: "Time waiting for merge to start",
  approved: "Time between approval and next action",
  pending_review: "Time waiting for review to start",
  revision_needed: "Time waiting for revision to begin",
};

/** Format snake_case phase name to Title Case (e.g. "merge_conflict" → "Merge Conflict") */
function formatPhaseName(phase: string): string {
  return phase
    .split("_")
    .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
    .join(" ");
}

function CycleTimeBar({ phase, maxMinutes }: { phase: CycleTimePhase; maxMinutes: number }) {
  const pct = maxMinutes > 0 ? Math.round((phase.avgMinutes / maxMinutes) * 100) : 0;
  const displayName = formatPhaseName(phase.phase);

  return (
    <div className="flex items-center gap-2 text-[0.75rem]">
      <span
        className="w-32 shrink-0 flex items-center gap-1 text-text-secondary"
      >
        <span title={displayName}>
          {displayName}
        </span>
        {PHASE_TOOLTIPS[phase.phase] && (
          <TooltipProvider delayDuration={200}>
            <Tooltip>
              <TooltipTrigger asChild>
                <HelpCircle className="w-3 h-3 shrink-0 text-muted-foreground" />
              </TooltipTrigger>
              <TooltipContent side="top" className="max-w-[200px] text-xs">
                {PHASE_TOOLTIPS[phase.phase]}
              </TooltipContent>
            </Tooltip>
          </TooltipProvider>
        )}
      </span>
      <div
        className="flex-1 h-1.5 rounded-full"
        style={{ backgroundColor: "var(--overlay-weak)" }}
      >
        <div
          className="h-full rounded-full"
          style={{ width: `${pct}%`, backgroundColor: "var(--accent-primary)" }}
        />
      </div>
      <span
        className="w-16 text-right shrink-0 tabular-nums text-text-secondary"
      >
        {formatMinutesHuman(phase.avgMinutes)}
      </span>
    </div>
  );
}

// ============================================================================
// Composite sections
// ============================================================================

interface QualityBreakdownProps {
  stats: ProjectStats;
}

export function QualityBreakdown({ stats }: QualityBreakdownProps) {
  const hasRates = stats.agentTotalCount > 0 || stats.reviewTotalCount > 0;
  if (!hasRates) return null;

  return (
    <DetailCard>
      <div className="flex flex-col gap-3">
        <div className="flex items-center gap-1.5">
          <GitMerge size={14} className="text-text-muted" />
          <span
            className="text-[0.6875rem] font-semibold uppercase tracking-wider text-text-muted"
            style={{ letterSpacing: "0.08em" }}
          >
            Quality Breakdown
          </span>
        </div>
        {stats.agentTotalCount > 0 && (
          <RateBar
            label="Agent success"
            rate={stats.agentSuccessRate}
            passCount={stats.agentSuccessCount}
            totalCount={stats.agentTotalCount}
            color="var(--accent-primary)"
          />
        )}
        {stats.reviewTotalCount > 0 && (
          <RateBar
            label="Review pass"
            rate={stats.reviewPassRate}
            passCount={stats.reviewPassCount}
            totalCount={stats.reviewTotalCount}
            color="var(--status-success)"
          />
        )}
      </div>
    </DetailCard>
  );
}

interface CycleTimeBreakdownProps {
  phases: CycleTimePhase[];
}

/** Terminal/non-pipeline states excluded from cycle time breakdown */
const TERMINAL_PHASES = new Set([
  "merged",
  "cancelled",
  "failed",
  "stopped",
  "paused",
  "blocked",
]);

export function CycleTimeBreakdown({ phases }: CycleTimeBreakdownProps) {
  const nonZeroPhases = phases.filter(
    (p) => p.avgMinutes >= 1 && !TERMINAL_PHASES.has(p.phase),
  );
  if (nonZeroPhases.length === 0) return null;
  const maxMinutes = nonZeroPhases.reduce((acc, p) => Math.max(acc, p.avgMinutes), 0);

  return (
    <DetailCard>
      <div className="flex flex-col gap-3">
        <div className="flex items-center gap-1.5">
          <Clock size={14} className="text-text-muted" />
          <span
            className="text-[0.6875rem] font-semibold uppercase tracking-wider text-text-muted"
            style={{ letterSpacing: "0.08em" }}
          >
            Cycle Time Breakdown
          </span>
        </div>
        <div className="grid grid-cols-1 lg:grid-cols-2 gap-2">
          {nonZeroPhases.map((phase) => (
            <CycleTimeBar key={phase.phase} phase={phase} maxMinutes={maxMinutes} />
          ))}
        </div>
      </div>
    </DetailCard>
  );
}

// ============================================================================
// Column Dwell Time Breakdown
// ============================================================================

function DwellTimeBar({ dwell, maxMinutes }: { dwell: ColumnDwellTime; maxMinutes: number }) {
  const pct = maxMinutes > 0 ? Math.round((dwell.avgMinutes / maxMinutes) * 100) : 0;

  return (
    <div className="flex items-center gap-2 text-[0.75rem]">
      <span
        className="w-24 shrink-0 truncate text-text-secondary"
        title={dwell.columnName}
      >
        {dwell.columnName}
      </span>
      <div
        className="flex-1 h-1.5 rounded-full"
        style={{ backgroundColor: "var(--overlay-weak)" }}
      >
        <div
          className="h-full rounded-full"
          style={{ width: `${pct}%`, backgroundColor: "var(--status-success)" }}
        />
      </div>
      <span
        className="w-16 text-right shrink-0 tabular-nums text-text-secondary"
      >
        {formatMinutesHuman(dwell.avgMinutes)}
      </span>
    </div>
  );
}

interface ColumnDwellTimeBreakdownProps {
  dwellTimes: ColumnDwellTime[];
}

export function ColumnDwellTimeBreakdown({ dwellTimes }: ColumnDwellTimeBreakdownProps) {
  const nonZero = dwellTimes.filter(
    (d) => d.avgMinutes > 0 && d.columnName.toLowerCase() !== "done"
  );
  if (nonZero.length === 0) return null;
  const maxMinutes = nonZero.reduce((acc, d) => Math.max(acc, d.avgMinutes), 0);

  return (
    <DetailCard>
      <div className="flex flex-col gap-3">
        <div className="flex items-center gap-1.5">
          <Columns3 size={14} className="text-text-muted" />
          <span
            className="text-[0.6875rem] font-semibold uppercase tracking-wider text-text-muted"
            style={{ letterSpacing: "0.08em" }}
          >
            Kanban Column Time
          </span>
        </div>
        <div className="grid grid-cols-1 lg:grid-cols-2 gap-2">
          {nonZero.map((dwell) => (
            <DwellTimeBar key={dwell.columnId} dwell={dwell} maxMinutes={maxMinutes} />
          ))}
        </div>
      </div>
    </DetailCard>
  );
}

// ============================================================================
// Markdown generation + copy button
// ============================================================================

function generateMarkdown(stats: ProjectStats): string {
  const date = new Date().toLocaleDateString("en-US", {
    year: "numeric",
    month: "long",
    day: "numeric",
  });

  const agentPct = Math.round(stats.agentSuccessRate * 100);
  const reviewPct = Math.round(stats.reviewPassRate * 100);

  const pipelinePhases = stats.cycleTimeBreakdown.filter(
    (p) => p.avgMinutes >= 1 && !TERMINAL_PHASES.has(p.phase),
  );
  const cycleTimeLine =
    pipelinePhases.length > 0
      ? pipelinePhases
          .map((p) => `${p.phase}: ${formatMinutesHuman(p.avgMinutes)}`)
          .join(", ")
      : "No data yet";

  const emeSection = stats.eme
    ? `\n### Estimated Manual Effort\n- **Estimate:** ~${stats.eme.lowHours}–${stats.eme.highHours} active hours${stats.eme.earliestTaskDate && stats.eme.latestTaskDate ? ` (${stats.eme.earliestTaskDate} to ${stats.eme.latestTaskDate})` : ""}\n- **Tasks analyzed:** ${stats.eme.taskCount}\n- *Based on task complexity analysis. Equivalent manual effort without AI.*\n`
    : "";

  return `## Project Stats — ${date}

### Quality
- **Agent Success Rate:** ${agentPct}% (${stats.agentSuccessCount}/${stats.agentTotalCount} tasks)
- **Review Pass Rate:** ${reviewPct}% first-pass approval

### Throughput
- **Tasks Completed:** ${stats.tasksCompletedThisWeek} this week, ${stats.tasksCompletedThisMonth} this month
- **Avg Cycle Time:** ${cycleTimeLine}
${emeSection}
---
*Metrics computed locally from RalphX — your data never leaves your machine.*`;
}

interface CopyMarkdownButtonProps {
  stats: ProjectStats;
}

export function CopyMarkdownButton({ stats }: CopyMarkdownButtonProps) {
  const [copied, setCopied] = useState(false);

  async function handleCopy() {
    try {
      await navigator.clipboard.writeText(generateMarkdown(stats));
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // Clipboard not available
    }
  }

  return (
    <button
      onClick={handleCopy}
      className="flex items-center gap-2 rounded-lg px-3 py-2 text-[0.75rem] font-medium transition-colors"
      style={{
        backgroundColor: "var(--bg-surface)",
        color: "var(--text-secondary)",
        border: "1px solid var(--overlay-faint)",
      }}
    >
      {copied ? (
        <>
          <Check size={13} style={{ color: "var(--status-success)" }} />
          <span style={{ color: "var(--status-success)" }}>Copied!</span>
        </>
      ) : (
        <>
          <Copy size={13} />
          Copy as Markdown
        </>
      )}
    </button>
  );
}
