/**
 * PlanGroupHeader.tsx - Header for plan groups in the Task Graph
 *
 * Two layouts:
 * - Collapsed: two rows (title + count, progress bar)
 * - Expanded: two rows (toggle + title + badges, branch badge + progress bar)
 *
 * Shows feature branch badge and settings gear when a plan branch exists.
 */

import { memo, useEffect, useMemo, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  ChevronDown,
  ChevronRight,
  Check,
  Play,
  AlertTriangle,
  Eye,
  GitMerge,
  GitBranch,
  Settings,
  X,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import type { StatusSummary } from "@/api/task-graph.types";
import { api } from "@/lib/tauri";
import { useEventBus } from "@/providers/EventProvider";
import { PlanGroupSettings } from "./PlanGroupSettings";

// ============================================================================
// Types
// ============================================================================

export interface PlanGroupHeaderProps {
  /** Plan artifact ID for linking */
  planArtifactId: string;
  /** Project ID for API calls */
  projectId?: string;
  /** Session/plan title to display */
  sessionTitle: string | null;
  /** Number of tasks in this plan group */
  taskCount: number;
  /** Status summary with counts by category */
  statusSummary: StatusSummary;
  /** Whether the group is collapsed */
  isCollapsed: boolean;
  /** Tier group IDs within this plan */
  tierGroupIds?: string[];
  /** Whether any tiers are collapsed */
  anyTierCollapsed?: boolean;
  /** Whether all tiers are collapsed */
  allTiersCollapsed?: boolean;
  /** Toggle all tiers expand/collapse */
  onToggleAllTiers?: (planArtifactId: string, action: "expand" | "collapse") => void;
  /** Toggle collapse state */
  onToggleCollapse: () => void;
  /** Optional: Open context menu */
  onContextMenu?: () => void;
  /** Optional: Navigate to planning session */
  onNavigateToSession?: () => void;
  /** Optional: Navigate to a specific task (merge task link) */
  onNavigateToTask?: (taskId: string) => void;
}

// ============================================================================
// Reusable Components
// ============================================================================

/** Collapse/expand toggle button */
const CollapseToggle = memo(function CollapseToggle({
  isCollapsed,
  onClick,
}: {
  isCollapsed: boolean;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      className="flex-shrink-0 p-0.5 rounded hover:bg-[var(--bg-surface)] transition-colors"
      aria-label={isCollapsed ? "Expand group" : "Collapse group"}
    >
      {isCollapsed ? (
        <ChevronRight className="w-4 h-4 text-[var(--text-muted)]" />
      ) : (
        <ChevronDown className="w-4 h-4 text-[var(--text-muted)]" />
      )}
    </button>
  );
});

/** Progress bar with percentage - thin and subtle */
const ProgressBar = memo(function ProgressBar({
  completed,
  total,
}: {
  completed: number;
  total: number;
}) {
  const percentage = total > 0 ? Math.round((completed / total) * 100) : 0;

  return (
    <div className="flex items-center gap-1.5 min-w-[100px]">
      <div className="flex-1 h-[2px] bg-[color-mix(in_srgb,_var(--border-subtle)_50%,_transparent)] rounded-full overflow-hidden">
        <div
          className="h-full bg-[color-mix(in_srgb,_var(--status-success)_60%,_transparent)] rounded-full transition-all duration-300"
          style={{ width: `${percentage}%` }}
        />
      </div>
      <span className="text-xs text-[var(--text-muted)] whitespace-nowrap">
        {percentage}%
      </span>
    </div>
  );
});

/** Status badge with icon and count */
const StatusBadge = memo(function StatusBadge({
  icon,
  count,
  label,
  colorClass,
}: {
  icon: React.ReactNode;
  count: number;
  label: string;
  colorClass: string;
}) {
  if (count === 0) return null;

  return (
    <div
      className={cn(
        "flex items-center gap-1 px-1.5 py-0.5 rounded text-xs font-medium",
        colorClass
      )}
      title={label}
    >
      {icon}
      <span>{count}</span>
    </div>
  );
});

/** Feature branch inline badge - compact indicator */
const FeatureBranchBadge = memo(function FeatureBranchBadge({
  branchName,
  status,
}: {
  branchName: string;
  status: "active" | "merged" | "abandoned";
}) {
  const statusColor = status === "active"
    ? "var(--status-success)"
    : status === "merged"
    ? "var(--status-info)"
    : "var(--text-muted)";

  const statusIcon = status === "active"
    ? <span className="w-1.5 h-1.5 rounded-full" style={{ backgroundColor: statusColor }} />
    : status === "merged"
    ? <Check className="w-2.5 h-2.5" />
    : <X className="w-2.5 h-2.5" />;

  // Abbreviate: last segment, truncated to 12 chars
  const lastSegment = branchName.split("/").pop() ?? branchName;
  const shortName = lastSegment.length > 12
    ? lastSegment.slice(0, 12) + "\u2026"
    : lastSegment;

  return (
    <div
      className="flex items-center gap-0.5 px-1 py-px rounded text-[0.5625rem] font-mono whitespace-nowrap flex-shrink-0 bg-[var(--bg-surface)]"
      style={{ color: statusColor }}
      title={`Feature branch: ${branchName} (${status})`}
    >
      <GitBranch className="w-2.5 h-2.5 flex-shrink-0" />
      {statusIcon}
      <span>{shortName}</span>
    </div>
  );
});

// ============================================================================
// Main Component
// ============================================================================

export const PlanGroupHeader = memo(function PlanGroupHeader({
  planArtifactId,
  projectId,
  sessionTitle,
  taskCount,
  statusSummary,
  isCollapsed,
  tierGroupIds,
  anyTierCollapsed,
  onToggleAllTiers,
  onToggleCollapse,
  onNavigateToSession,
  onNavigateToTask,
}: PlanGroupHeaderProps) {
  const queryClient = useQueryClient();
  const eventBus = useEventBus();
  const [settingsOpen, setSettingsOpen] = useState(false);

  // Fetch plan branch data
  const { data: planBranch } = useQuery({
    queryKey: ["plan-branch", planArtifactId],
    queryFn: () => api.planBranches.getByPlan(planArtifactId),
    enabled: Boolean(planArtifactId),
    staleTime: 5_000,
  });


  // Reactively update badge when plan merge completes
  useEffect(() => {
    const unsubscribe = eventBus.subscribe("plan:merge_complete", () => {
      queryClient.invalidateQueries({ queryKey: ["plan-branch", planArtifactId] });
    });
    return unsubscribe;
  }, [eventBus, queryClient, planArtifactId]);

  const counts = useMemo(() => ({
    done: statusSummary.completed,
    executing: statusSummary.executing,
    blocked: statusSummary.blocked,
    review: statusSummary.review,
    merge: statusSummary.merge,
    total: taskCount,
  }), [statusSummary, taskCount]);

  const displayTitle = sessionTitle || "Unnamed Plan";
  const hasTiers = (tierGroupIds?.length ?? 0) > 0;
  const shouldExpandAll = Boolean(anyTierCollapsed);

  // Collapsed: two-row layout with text count
  if (isCollapsed) {
    return (
      <div className="flex flex-col gap-1.5 px-3 py-2 bg-[color-mix(in_srgb,_var(--bg-elevated)_80%,_transparent)] rounded-lg cursor-pointer">
        {/* Row 1: toggle + title + branch badge + count */}
        <div className="flex items-center gap-2">
          <CollapseToggle isCollapsed={true} onClick={onToggleCollapse} />
          <span
            className="text-sm font-medium text-[var(--text-primary)] truncate flex-1"
            title={`Plan: ${displayTitle}`}
          >
            {displayTitle}
          </span>
          {planBranch && planBranch.status !== "abandoned" && (
            <FeatureBranchBadge
              branchName={planBranch.branchName}
              status={planBranch.status}
            />
          )}
          <span className="text-xs text-[var(--text-muted)]">
            {counts.total} tasks
          </span>
        </div>
        {/* Row 2: progress bar */}
        <ProgressBar completed={counts.done} total={counts.total} />
      </div>
    );
  }

  // Expanded: two-row layout
  return (
    <div className="flex flex-col gap-1 px-2 py-1.5 bg-[color-mix(in_srgb,_var(--bg-elevated)_80%,_transparent)] rounded-t-lg cursor-pointer">
      {/* Row 1: toggle + title + tier toggle + status badges + settings */}
      <div className="flex items-center justify-between gap-2 w-full">
        <div className="flex items-center gap-1.5 min-w-0 flex-1">
          <CollapseToggle isCollapsed={false} onClick={onToggleCollapse} />
          <span
            className={cn(
              "text-sm font-medium text-[var(--text-primary)] truncate",
              onNavigateToSession &&
                "hover:text-[var(--accent-primary)] transition-colors cursor-pointer"
            )}
            title={`Plan: ${displayTitle}`}
            onClick={onNavigateToSession}
          >
            {displayTitle}
          </span>
        </div>

        <div className="flex items-center gap-1 flex-shrink-0">
          {/* Tier toggle */}
          {hasTiers && (
            <button
              className={cn(
                "flex-shrink-0 p-0.5 rounded transition-colors",
                "hover:bg-[var(--bg-surface)]",
              )}
              onClick={(event) => {
                event.stopPropagation();
                onToggleAllTiers?.(
                  planArtifactId,
                  shouldExpandAll ? "expand" : "collapse"
                );
              }}
              aria-label={shouldExpandAll ? "Expand all tiers" : "Collapse all tiers"}
              title={shouldExpandAll ? "Expand all tiers" : "Collapse all tiers"}
            >
              {shouldExpandAll ? (
                <ChevronRight className="w-3.5 h-3.5 text-[var(--text-muted)]" />
              ) : (
                <ChevronDown className="w-3.5 h-3.5 text-[var(--text-muted)]" />
              )}
            </button>
          )}

          {/* Status badges */}
          <StatusBadge
            icon={<Check className="w-3 h-3" />}
            count={counts.done}
            label={`${counts.done} completed`}
            colorClass="bg-[var(--status-success-muted)] text-[var(--status-success)]"
          />
          <StatusBadge
            icon={<Play className="w-3 h-3" />}
            count={counts.executing}
            label={`${counts.executing} executing`}
            colorClass="bg-[var(--accent-muted)] text-[var(--accent-primary)]"
          />
          <StatusBadge
            icon={<AlertTriangle className="w-3 h-3" />}
            count={counts.blocked}
            label={`${counts.blocked} blocked`}
            colorClass="bg-[var(--status-warning-muted)] text-[var(--status-warning)]"
          />
          <StatusBadge
            icon={<Eye className="w-3 h-3" />}
            count={counts.review}
            label={`${counts.review} in review`}
            colorClass="bg-[var(--status-info-muted)] text-[var(--status-info)]"
          />
          <StatusBadge
            icon={<GitMerge className="w-3 h-3" />}
            count={counts.merge}
            label={`${counts.merge} merging`}
            colorClass="bg-[var(--status-info-muted)] text-[var(--status-info)]"
          />

          {/* Settings gear - opens PlanGroupSettings popover */}
          {projectId && (
            <Popover open={settingsOpen} onOpenChange={setSettingsOpen}>
              <PopoverTrigger asChild>
                <button
                  className={cn(
                    "p-1 rounded transition-colors",
                    "hover:bg-[var(--bg-surface)]",
                    settingsOpen && "bg-[var(--bg-surface)]"
                  )}
                  onClick={(event) => event.stopPropagation()}
                  aria-label="Plan group settings"
                  title="Feature branch settings"
                >
                  <Settings className="w-3.5 h-3.5 text-[var(--text-muted)]" />
                </button>
              </PopoverTrigger>
              <PopoverContent
                align="end"
                sideOffset={8}
                className="w-auto p-3"
                onClick={(event) => event.stopPropagation()}
              >
                <PlanGroupSettings
                  planBranch={planBranch ?? null}
                  onNavigateToMergeTask={(taskId) => {
                    setSettingsOpen(false);
                    onNavigateToTask?.(taskId);
                  }}
                />
              </PopoverContent>
            </Popover>
          )}
        </div>
      </div>

      {/* Row 2: branch badge + merge target + progress bar */}
      <div className="flex items-center gap-2 pl-6 w-full">
        {planBranch && planBranch.status !== "abandoned" && (
          <FeatureBranchBadge
            branchName={planBranch.branchName}
            status={planBranch.status}
          />
        )}
        {planBranch && planBranch.baseBranchOverride && (
          <div
            className="flex items-center gap-0.5 px-1 py-px rounded text-[0.5625rem] font-mono whitespace-nowrap flex-shrink-0 bg-[var(--bg-surface)] text-[var(--text-muted)]"
            title={`Merging into: ${planBranch.baseBranchOverride}`}
          >
            <span style={{ opacity: 0.5 }}>→</span>
            <span>{planBranch.baseBranchOverride}</span>
          </div>
        )}
        <ProgressBar completed={counts.done} total={counts.total} />
      </div>
    </div>
  );
});
