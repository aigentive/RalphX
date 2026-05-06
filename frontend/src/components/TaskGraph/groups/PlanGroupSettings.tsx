/**
 * PlanGroupSettings.tsx - Settings popover for plan groups in the Task Graph
 *
 * Shows feature branch toggle, branch info, status, and merge task link.
 * Opens from the gear icon in PlanGroupHeader.
 */

import { memo } from "react";
import {
  GitBranch,
  Check,
  X,
  ExternalLink,
} from "lucide-react";
import { cn } from "@/lib/utils";
import type { PlanBranch } from "@/api/plan-branch.types";

// ============================================================================
// Types
// ============================================================================

export interface PlanGroupSettingsProps {
  /** Current plan branch data (null if no feature branch) */
  planBranch: PlanBranch | null;
  /** Navigate to merge task in graph */
  onNavigateToMergeTask?: (taskId: string) => void;
}

// ============================================================================
// Status indicator component
// ============================================================================

const BranchStatusIndicator = memo(function BranchStatusIndicator({
  status,
}: {
  status: PlanBranch["status"];
}) {
  switch (status) {
    case "active":
      return (
        <span className="inline-flex items-center gap-1 text-[0.6875rem] text-[var(--status-success)]">
          <span className="w-1.5 h-1.5 rounded-full bg-[var(--status-success)]" />
          Active
        </span>
      );
    case "merged":
      return (
        <span className="inline-flex items-center gap-1 text-[0.6875rem] text-[var(--status-info)]">
          <Check className="w-3 h-3" />
          Merged
        </span>
      );
    case "abandoned":
      return (
        <span className="inline-flex items-center gap-1 text-[0.6875rem] text-[var(--text-muted)]">
          <X className="w-3 h-3" />
          Abandoned
        </span>
      );
  }
});

// ============================================================================
// Main Component
// ============================================================================

export const PlanGroupSettings = memo(function PlanGroupSettings({
  planBranch,
  onNavigateToMergeTask,
}: PlanGroupSettingsProps) {
  return (
    <div className="flex flex-col gap-3 min-w-[240px]">
      {/* Header */}
      <div className="flex items-center gap-2">
        <GitBranch className="w-3.5 h-3.5 text-[var(--text-muted)]" />
        <span className="text-xs font-medium text-[var(--text-primary)]">
          Feature Branch
        </span>
      </div>

      {/* Branch info (when enabled) */}
      {planBranch && (
        <div className="flex flex-col gap-2 pt-2 border-t border-[var(--border-subtle)]">
          {/* Branch name */}
          <div className="flex flex-col gap-0.5">
            <span className="text-[0.625rem] uppercase tracking-wider text-[var(--text-muted)]">
              Branch
            </span>
            <span
              className={cn(
                "text-[0.6875rem] font-mono text-[var(--text-secondary)]",
                "bg-[var(--bg-surface)] px-1.5 py-0.5 rounded"
              )}
            >
              {planBranch.branchName}
            </span>
          </div>

          {/* Status */}
          <div className="flex items-center justify-between">
            <span className="text-[0.625rem] uppercase tracking-wider text-[var(--text-muted)]">
              Status
            </span>
            <BranchStatusIndicator status={planBranch.status} />
          </div>

          {/* Source branch */}
          <div className="flex items-center justify-between">
            <span className="text-[0.625rem] uppercase tracking-wider text-[var(--text-muted)]">
              Source
            </span>
            <span className="text-[0.6875rem] font-mono text-[var(--text-muted)]">
              {planBranch.sourceBranch}
            </span>
          </div>

          {/* Merge target */}
          {planBranch.baseBranchOverride && (
            <div className="flex items-center justify-between">
              <span className="text-[0.625rem] uppercase tracking-wider text-[var(--text-muted)]">
                Merge Target
              </span>
              <span className="text-[0.6875rem] font-mono text-[var(--text-muted)]">
                {planBranch.baseBranchOverride}
              </span>
            </div>
          )}

          {/* Merge task link */}
          {planBranch.mergeTaskId && onNavigateToMergeTask && (
            <button
              className={cn(
                "flex items-center gap-1.5 mt-1",
                "text-[0.6875rem] text-[var(--accent-primary)]",
                "hover:underline cursor-pointer"
              )}
              onClick={() => onNavigateToMergeTask(planBranch.mergeTaskId!)}
            >
              <ExternalLink className="w-3 h-3" />
              View merge task
            </button>
          )}
        </div>
      )}

    </div>
  );
});
