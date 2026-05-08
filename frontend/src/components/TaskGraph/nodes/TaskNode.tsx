/**
 * TaskNode - Custom React Flow node for task visualization
 *
 * Primary task node (180px width) with:
 * - Status-based border and background colors
 * - Truncated task title
 * - Status badge
 * - Handles for connections (top source, bottom target)
 * - Right-click context menu with status-appropriate actions
 *
 * Per spec: Phase B.2 + E.2 of Task Graph View implementation
 */

import { memo, useCallback } from "react";
import { Handle, Position, type NodeProps, type Node } from "@xyflow/react";
import { useScaledNodeDimensions } from "./nodeStyles";
import { TaskNodeContextMenu } from "./TaskNodeContextMenu";
import { useStepProgress } from "@/hooks/useTaskSteps";
import type { InternalStatus } from "@/types/status";
import { TaskStatusBadge } from "@/components/tasks/TaskBoard/TaskStatusBadge";
import { getBaseCardStyles } from "@/components/tasks/TaskBoard/TaskCard.utils";
import { getTaskCategoryLabel } from "@/lib/task-category";
import type { Task } from "@/types/task";
import type { GroupInfo } from "@/lib/task-actions";

// ============================================================================
// Types
// ============================================================================

/**
 * Handler functions passed to TaskNode for context menu actions
 */
export interface TaskNodeHandlers {
  /** Open the task detail overlay */
  onViewDetails: (taskId: string) => void;
  /** Start task execution (ready status) */
  onStartExecution?: (taskId: string) => void;
  /** Block a task with optional reason */
  onBlockWithReason?: (taskId: string, reason?: string) => void;
  /** Unblock a task */
  onUnblock?: (taskId: string) => void;
  /** Approve a task */
  onApprove?: (taskId: string) => void;
  /** Reject a task */
  onReject?: (taskId: string) => void;
  /** Request changes */
  onRequestChanges?: (taskId: string) => void;
  /** Mark merge conflict as resolved */
  onMarkResolved?: (taskId: string) => void;
  /** Remove task (full cleanup) */
  onRemove?: (taskId: string) => void;
}

/**
 * Data passed to the TaskNode component
 * Uses Record<string, unknown> intersection to satisfy React Flow constraints
 */
export type TaskNodeData = Record<string, unknown> & {
  label: string;
  taskId: string;
  internalStatus: string;
  priority: number;
  isCriticalPath: boolean;
  /** Task description for compact display (2-line clamp) */
  description?: string | null;
  /** Task category for badge display */
  category?: string;
  /** Target branch for plan_merge category nodes (e.g., 'develop', 'main') */
  mergeTarget?: string;
  /** Pull request number for plan_merge nodes */
  prNumber?: number | null;
  /** Pull request status for plan_merge nodes */
  prStatus?: string | null;
  /** Plan branch lifecycle status for plan_merge nodes */
  planBranchStatus?: string;
  /** Whether this node is highlighted (e.g., from timeline click) */
  isHighlighted?: boolean;
  /** Whether this node is keyboard-focused (for keyboard navigation) */
  isFocused?: boolean;
  /** Handler functions for context menu actions */
  handlers?: TaskNodeHandlers;
  /** Group context for showing group actions in task context menu */
  groupInfo?: GroupInfo;
};

export type TaskNodeType = Node<TaskNodeData, "task">;

// ============================================================================
// Component
// ============================================================================

/**
 * Get background color for step dot based on status (inline style)
 *
 * Color scheme:
 * - Completed: light gray (or green when task is in merged/approved terminal state)
 * - Skipped: muted gray
 * - Failed: red
 * - In-progress: orange
 * - Pending: darker gray
 */
function getStepDotStyle(
  index: number,
  completed: number,
  skipped: number,
  failed: number,
  inProgress: number,
  isTerminalComplete: boolean
): React.CSSProperties {
  const completedAndSkipped = completed + skipped;
  const failedStart = completedAndSkipped;
  const failedEnd = failedStart + failed;
  const inProgressStart = failedEnd;
  const inProgressEnd = inProgressStart + inProgress;

  const base: React.CSSProperties = {
    width: 6,
    height: 6,
    borderRadius: "50%",
  };

  // Green only for terminal complete states (merged/approved)
  const completedColor = isTerminalComplete
    ? "var(--status-success)" // success green
    : "var(--text-muted)"; // light gray

  if (index < completed) return { ...base, backgroundColor: completedColor };
  if (index < completedAndSkipped) return { ...base, backgroundColor: "var(--text-muted)" }; // muted
  if (index < failedEnd) return { ...base, backgroundColor: "var(--status-error)" }; // error red
  if (index < inProgressEnd) return { ...base, backgroundColor: "var(--accent-primary)" }; // accent orange
  return { ...base, backgroundColor: "var(--border-subtle)" }; // pending darker gray
}


function TaskNodeComponent({ data, selected }: NodeProps<TaskNodeType>) {
  const { label, taskId, internalStatus, priority, isCriticalPath, description, category, mergeTarget, prNumber, prStatus, planBranchStatus, isHighlighted, isFocused, handlers, groupInfo } = data;
  const mergeLabel = mergeTarget ? `Merge -> ${mergeTarget}` : "Merge";
  const categoryLabel = category === "plan_merge" ? mergeLabel : getTaskCategoryLabel(category);
  const hasPrContext = category === "plan_merge" && typeof prNumber === "number";
  const prState =
    planBranchStatus === "merged" || internalStatus === "merged" || prStatus === "Merged"
      ? "merged"
      : prStatus === "Closed"
      ? "closed"
      : "review";
  const prLabel =
    prState === "merged"
      ? `PR #${prNumber} merged`
      : prState === "closed"
      ? `PR #${prNumber} closed`
      : `PR #${prNumber}`;
  const prColor =
    prState === "merged"
      ? "var(--status-success)"
      : prState === "closed"
      ? "var(--status-warning)"
      : "var(--status-info)";
  const prTitle =
    prState === "merged"
      ? `PR #${prNumber} has been merged`
      : prState === "closed"
      ? `PR #${prNumber} was closed`
      : `PR #${prNumber} is waiting for GitHub review or merge`;
  const cardSurface = getBaseCardStyles(internalStatus as InternalStatus, false, false);
  const { nodeWidth, nodeHeight } = useScaledNodeDimensions();
  const { data: stepProgress } = useStepProgress(taskId);
  const isTerminalComplete = internalStatus === "merged" || internalStatus === "approved";

  // Create a minimal task-like object for the context menu
  // The context menu only uses title and internalStatus
  const minimalTask: Task = {
    id: taskId,
    projectId: "", // Not needed for context menu
    category: category ?? "",
    title: label,
    description: description ?? null,
    priority,
    internalStatus: internalStatus as InternalStatus,
    needsReviewPoint: false,
    createdAt: "",
    updatedAt: "",
    startedAt: null,
    completedAt: null,
    archivedAt: null,
    blockedReason: null,
  };

  // Wrap handlers to pass taskId
  const handleViewDetails = useCallback(() => {
    handlers?.onViewDetails(taskId);
  }, [handlers, taskId]);

  const handleStartExecution = useCallback(() => {
    handlers?.onStartExecution?.(taskId);
  }, [handlers, taskId]);

  const handleBlockWithReason = useCallback((reason?: string) => {
    handlers?.onBlockWithReason?.(taskId, reason);
  }, [handlers, taskId]);

  const handleUnblock = useCallback(() => {
    handlers?.onUnblock?.(taskId);
  }, [handlers, taskId]);

  const handleApprove = useCallback(() => {
    handlers?.onApprove?.(taskId);
  }, [handlers, taskId]);

  const handleReject = useCallback(() => {
    handlers?.onReject?.(taskId);
  }, [handlers, taskId]);

  const handleRequestChanges = useCallback(() => {
    handlers?.onRequestChanges?.(taskId);
  }, [handlers, taskId]);

  const handleMarkResolved = useCallback(() => {
    handlers?.onMarkResolved?.(taskId);
  }, [handlers, taskId]);

  const handleRemove = useCallback(() => {
    handlers?.onRemove?.(taskId);
  }, [handlers, taskId]);

  // Node content that will be wrapped by context menu
  const nodeContent = (
    <div
      className="relative"
      style={{ width: nodeWidth }}
      data-testid="task-node"
      data-status={internalStatus}
      data-critical-path={isCriticalPath}
      data-highlighted={isHighlighted}
      data-focused={isFocused}
    >
      {/* Target handle - top (incoming edges) */}
      <Handle
        type="target"
        position={Position.Top}
        className="!bg-[var(--text-muted)] !border-[var(--border-default)] !w-1.5 !h-1.5 !opacity-50 hover:!opacity-100 transition-opacity duration-150"
        style={{ top: -3 }}
      />

      {/* Node content - Glass morphism surface with fixed height */}
      <div
        className={`
          relative rounded-lg px-3 py-2 overflow-hidden
          transition-all duration-150 ease-out
          hover:shadow-lg
          ${isCriticalPath && !selected && !isHighlighted && !isFocused ? "ring-1 ring-[var(--accent-border)]" : ""}
        `}
        style={{
          height: nodeHeight - 6,
          backgroundColor: cardSurface.backgroundColor,
          borderStyle: "solid",
          borderWidth: (selected || isHighlighted || isFocused) ? "2px" : "1px",
          borderColor: (selected || isHighlighted || isFocused)
            ? "var(--accent-primary)"
            : (cardSurface.borderColor as string),
          boxShadow: "none",
          transition: "background-color 150ms ease, transform 150ms ease, box-shadow 150ms ease, border-color 150ms ease",
        }}
      >
        {/* Status badge - top-right corner (shared with Kanban) */}
        <div className="absolute top-1 right-1" data-testid="status-badge-container">
          <TaskStatusBadge status={internalStatus as InternalStatus} />
        </div>

        {/* Title - merge popover typography parity - 2-line clamp */}
        <div
          className="leading-tight pr-7 line-clamp-2"
          style={{
            fontSize: "0.75rem",
            fontWeight: 500,
            color: "var(--text-primary)",
            lineHeight: 1.35,
            minHeight: "17px",
            maxHeight: "34px",
          }}
          title={label}
        >
          {label}
        </div>

        {/* Description area - 2-line clamp */}
        <div
          className="mt-1 pr-2"
          style={{
            minHeight: "17px",
            maxHeight: "35px",
          }}
        >
          {description && (
            <div
              className="line-clamp-2"
              style={{
                fontSize: "0.6875rem",
                color: "var(--text-muted)",
                lineHeight: 1.4,
              }}
            >
              {description}
            </div>
          )}
        </div>

        {/* Category, PR context, and step dots - same line */}
        <div
          className="flex items-center gap-2 mt-2 min-w-0"
          style={{ height: "16px" }}
          data-testid="step-progress-footer"
        >
          {category && (
            <span
              style={{
                fontSize: "0.625rem",
                fontWeight: 500,
                color: "var(--text-muted)",
                textTransform: "capitalize",
                overflow: "hidden",
                textOverflow: "ellipsis",
                whiteSpace: "nowrap",
                minWidth: 0,
              }}
              title={categoryLabel}
            >
              {categoryLabel}
            </span>
          )}
          {hasPrContext && (
            <span
              data-testid="graph-pr-indicator"
              style={{
                fontSize: "0.625rem",
                fontWeight: 500,
                color: prColor,
                whiteSpace: "nowrap",
                flexShrink: 0,
              }}
              title={prTitle}
            >
              {prLabel}
            </span>
          )}
          {/* Show dots when we have step data */}
          {stepProgress && stepProgress.total > 0 && (
            <div style={{ display: "flex", alignItems: "center", gap: 4 }}>
              {Array.from({ length: stepProgress.total }).map((_, index) => (
                <div
                  key={index}
                  style={getStepDotStyle(
                    index,
                    stepProgress.completed,
                    stepProgress.skipped,
                    stepProgress.failed,
                    stepProgress.inProgress,
                    isTerminalComplete
                  )}
                />
              ))}
            </div>
          )}
        </div>

        {/* Progress bar */}
        {stepProgress && stepProgress.total > 0 && (
          <div className="flex items-center gap-2 mt-1">
            <div
              className="flex-1 h-1 rounded-full overflow-hidden"
              style={{ backgroundColor: "var(--border-subtle)" }}
            >
              <div
                className="h-full rounded-full transition-all duration-300"
                style={{
                  width: `${Math.round(((stepProgress.completed + stepProgress.skipped) / stepProgress.total) * 100)}%`,
                  backgroundColor: "var(--text-muted)",
                }}
              />
            </div>
            <span
              className="text-[0.625rem] tabular-nums shrink-0"
              style={{ color: "var(--text-muted)" }}
            >
              {Math.round(((stepProgress.completed + stepProgress.skipped) / stepProgress.total) * 100)}%
            </span>
          </div>
        )}
      </div>

      {/* Source handle - bottom (outgoing edges) */}
      <Handle
        type="source"
        position={Position.Bottom}
        className="!bg-[var(--text-muted)] !border-[var(--border-default)] !w-1.5 !h-1.5 !opacity-50 hover:!opacity-100 transition-opacity duration-150"
        style={{ bottom: -3 }}
      />
    </div>
  );

  // If no handlers provided, render without context menu (for preview/compact modes)
  if (!handlers) {
    return nodeContent;
  }

  // Wrap with context menu
  return (
    <TaskNodeContextMenu
      task={minimalTask}
      handlers={{
        onViewDetails: handleViewDetails,
        onStartExecution: handleStartExecution,
        onBlockWithReason: handleBlockWithReason,
        onUnblock: handleUnblock,
        onViewAgentChat: handleViewDetails, // Falls back to view details
        onApprove: handleApprove,
        onReject: handleReject,
        onRequestChanges: handleRequestChanges,
        onMarkResolved: handleMarkResolved,
        onRemove: handleRemove,
      }}
      {...(groupInfo !== undefined && { groupInfo })}
    >
      {nodeContent}
    </TaskNodeContextMenu>
  );
}

/**
 * Memoized TaskNode for React Flow performance
 */
export const TaskNode = memo(TaskNodeComponent);
