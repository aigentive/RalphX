/**
 * TaskMetricsCard - Shows per-task engineering metrics in a detail view.
 *
 * Displays step count, review cycles, execution time, and derived complexity
 * tier. Fetches data via `useTaskMetrics` on mount (cached for 5 minutes).
 *
 * Complexity tier uses neutral task-detail chrome.
 */

import { Loader2, ListChecks, RotateCcw, Timer, Zap } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useTaskMetrics } from "@/hooks/useTaskMetrics";
import { deriveComplexityTier } from "@/api/task-metrics";

// ── Stat cell ────────────────────────────────────────────────────────────────

function StatCell({
  icon: Icon,
  label,
  value,
}: {
  icon: LucideIcon;
  label: string;
  value: string;
}) {
  return (
    <div className="flex items-start gap-2 min-w-0">
      <div
        className="flex items-center justify-center w-7 h-7 rounded-lg shrink-0"
        style={{ backgroundColor: "var(--overlay-weak)" }}
      >
        <Icon className="w-3.5 h-3.5 text-text-primary/40" />
      </div>
      <div className="min-w-0">
        <span className="text-[0.625rem] uppercase tracking-wider text-text-primary/40 block">
          {label}
        </span>
        <span className="text-[0.8125rem] text-text-primary font-medium truncate block">
          {value}
        </span>
      </div>
    </div>
  );
}

// ── Helpers ──────────────────────────────────────────────────────────────────

function formatMinutes(minutes: number): string {
  if (minutes < 1) return "< 1 min";
  if (minutes < 60) return `${Math.round(minutes)} min`;
  const h = Math.floor(minutes / 60);
  const m = Math.round(minutes % 60);
  return m > 0 ? `${h}h ${m}m` : `${h}h`;
}

// ── Main component ────────────────────────────────────────────────────────────

interface TaskMetricsCardProps {
  taskId: string;
}

/**
 * TaskMetricsCard — fetches and displays per-task engineering metrics.
 *
 * Show this in MergedTaskDetail and WaitingTaskDetail where execution data
 * is meaningful. Do not render on cards — the badge is only visible after
 * the user opens the task detail (data is then cached for subsequent card renders).
 */
export function TaskMetricsCard({ taskId }: TaskMetricsCardProps) {
  const { data: metrics, isLoading, isError } = useTaskMetrics(taskId);

  if (isLoading) {
    return (
      <div className="flex items-center justify-center py-4">
        <Loader2
          className="w-4 h-4 animate-spin text-text-primary/30"
        />
      </div>
    );
  }

  if (isError || !metrics) {
    return null;
  }

  const tier = deriveComplexityTier(metrics);

  const rows = [
    {
      icon: Zap,
      label: "Complexity",
      value: tier,
    },
    {
      icon: ListChecks,
      label: "Steps",
      value:
        metrics.stepCount > 0
          ? `${metrics.completedStepCount} / ${metrics.stepCount} completed`
          : "No steps",
    },
    {
      icon: RotateCcw,
      label: "Reviews",
      value:
        metrics.reviewCount > 0
          ? `${metrics.reviewCount} cycle${metrics.reviewCount !== 1 ? "s" : ""}`
          : "No reviews",
    },
    {
      icon: Timer,
      label: "Execution time",
      value: metrics.executionMinutes > 0 ? formatMinutes(metrics.executionMinutes) : "—",
    },
  ];

  return (
    <div className="flex flex-wrap gap-x-8 gap-y-3">
      {rows.map((row) => (
        <StatCell
          key={row.label}
          icon={row.icon}
          label={row.label}
          value={row.value}
        />
      ))}
    </div>
  );
}
