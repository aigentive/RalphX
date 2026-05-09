/**
 * WaitingMergeCard - Compact row for merge waiting in queue
 *
 * Single-line row: clock icon | title | deferred badge | branch (short)
 * Deferred reason available via native tooltip.
 * Main-merge-deferred tasks show a distinct "Agents running" indicator.
 */

import { Clock, GitPullRequest, Users } from "lucide-react";
import type { MergePipelineTask } from "@/api/merge-pipeline";
import { getStatusIconConfig } from "@/types/status-icons";
import { BranchBadge } from "@/components/shared/BranchBadge";
interface WaitingMergeCardProps {
  task: MergePipelineTask;
  /** Number of currently running agents (shown for main-merge-deferred tasks) */
  runningCount?: number | undefined;
  onViewDetails: (taskId: string) => void;
}

export function WaitingMergeCard({ task, runningCount, onViewDetails }: WaitingMergeCardProps) {
  const pendingMergeStyle = getStatusIconConfig("pending_merge");
  const waitingOnPrStyle = getStatusIconConfig("waiting_on_pr");
  const isWaitingOnPr = task.internalStatus === "waiting_on_pr";

  const deferredReason = isWaitingOnPr
    ? "Waiting for GitHub PR review or merge"
    : task.isMainMergeDeferred
    ? runningCount && runningCount > 0
      ? `Waiting for ${runningCount} agent${runningCount === 1 ? "" : "s"} to finish`
      : "Waiting for agents to finish"
    : task.isDeferred
      ? task.blockingBranch
        ? `Waiting for ${task.blockingBranch} to merge`
        : "Waiting for active merge to complete"
      : "Pending merge";

  return (
    <div
      className="flex items-center gap-2 px-2 py-1.5 rounded-md hover:bg-white/[0.04] transition-colors"
      title={`${task.internalStatus} → ${task.targetBranch}\n${deferredReason}`}
    >
      {isWaitingOnPr ? (
        <GitPullRequest
          className="w-3.5 h-3.5 shrink-0"
          style={{ color: waitingOnPrStyle.color }}
        />
      ) : task.isMainMergeDeferred ? (
        <Users
          className="w-3.5 h-3.5 shrink-0"
          style={{ color: "var(--accent-primary)" }}
          data-testid="main-merge-deferred-icon"
        />
      ) : (
        <Clock
          className="w-3.5 h-3.5 shrink-0"
          style={{ color: pendingMergeStyle.color }}
        />
      )}
      <button
        className="flex-1 text-xs font-medium truncate min-w-0 text-left cursor-pointer hover:opacity-75 transition-opacity"
        style={{ color: task.isMainMergeDeferred ? "var(--text-secondary)" : "var(--text-secondary)" }}
        onClick={() => onViewDetails(task.taskId)}
      >
        {task.title}
      </button>
      {task.isMainMergeDeferred && (
        <span
          className="text-[0.625rem] shrink-0 px-1.5 py-0.5 rounded-full"
          style={{
            color: "var(--accent-primary)",
            backgroundColor: "var(--accent-muted)",
          }}
          data-testid="main-merge-deferred-badge"
        >
          {runningCount && runningCount > 0
            ? `${runningCount} agent${runningCount === 1 ? "" : "s"}`
            : "agents"}
        </span>
      )}
      <span className="shrink-0 max-w-[100px] truncate">
        <BranchBadge branch={task.targetBranch} variant="muted" size="sm" />
      </span>
    </div>
  );
}
