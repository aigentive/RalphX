/**
 * AttentionMergeCard - Compact row for merge needing attention
 *
 * Single-line row: icon | title | error summary | action buttons
 * Full details available via native tooltip on hover.
 */

import { AlertTriangle, ExternalLink, RotateCw } from "lucide-react";
import type { MergePipelineTask } from "@/api/merge-pipeline";
import { useCallback } from "react";
import { getStatusIconConfig } from "@/types/status-icons";

interface AttentionMergeCardProps {
  task: MergePipelineTask;
  onViewDetails: (taskId: string) => void;
  onRetry: (taskId: string) => void;
}

export function AttentionMergeCard({ task, onViewDetails, onRetry }: AttentionMergeCardProps) {
  const attentionStyle = getStatusIconConfig(task.internalStatus);
  const retryStyle = getStatusIconConfig("pending_merge");
  const handleViewDetails = useCallback(() => {
    onViewDetails(task.taskId);
  }, [onViewDetails, task.taskId]);

  const handleRetry = useCallback(() => {
    onRetry(task.taskId);
  }, [onRetry, task.taskId]);

  const errorContext = task.conflictFiles && task.conflictFiles.length > 0
    ? `${task.conflictFiles.length} conflicted files`
    : task.errorContext || "Git error occurred";

  const tooltipDetail = `${task.internalStatus} → ${task.targetBranch}\n${errorContext}`;

  return (
    <div
      className="flex items-center gap-2 px-2 py-1.5 rounded-md hover:bg-white/[0.04] transition-colors"
      title={tooltipDetail}
    >
      <AlertTriangle
        className="w-3.5 h-3.5 shrink-0"
        style={{ color: attentionStyle.color }}
      />
      <button
        className="flex-1 text-xs font-medium truncate min-w-0 text-left cursor-pointer hover:opacity-75 transition-opacity"
        style={{ color: "var(--text-primary)" }}
        onClick={handleViewDetails}
      >
        {task.displayTitle ?? task.title}
      </button>
      <span
        className="text-[0.6875rem] shrink-0 max-w-[120px] truncate"
        style={{ color: "var(--text-muted)" }}
      >
        {errorContext}
      </span>
      <div className="flex items-center shrink-0 ml-0.5">
        <button
          onClick={handleViewDetails}
          className="w-6 h-6 flex items-center justify-center rounded hover:bg-white/[0.08] transition-colors"
          style={{ color: "var(--text-muted)" }}
          title="View details"
        >
          <ExternalLink className="w-3 h-3" />
        </button>
        <button
          onClick={handleRetry}
          className="w-6 h-6 flex items-center justify-center rounded hover:bg-white/[0.08] transition-colors"
          style={{ color: retryStyle.color }}
          title="Retry merge"
        >
          <RotateCw className="w-3 h-3" />
        </button>
      </div>
    </div>
  );
}
