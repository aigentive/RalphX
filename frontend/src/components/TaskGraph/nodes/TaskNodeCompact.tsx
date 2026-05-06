/**
 * TaskNodeCompact - Compact React Flow node variant for large graphs
 *
 * Smaller node (COMPACT_NODE_WIDTH from nodeStyles.ts) for graphs with 50+ tasks:
 * - Status-based border and background colors
 * - Abbreviated task title (first 2 words + ellipsis)
 * - No status badge (status communicated via color)
 * - Handles for connections (top source, bottom target)
 * - Right-click context menu (same as TaskNode)
 *
 * Per spec: Phase F.1 of Task Graph View implementation
 */

import { memo, useCallback } from "react";
import { Handle, Position, type NodeProps, type Node } from "@xyflow/react";
import { COMPACT_TITLE_MAX_CHARS, useScaledNodeDimensions } from "./nodeStyles";
import { getBaseCardStyles } from "@/components/tasks/TaskBoard/TaskCard.utils";
import { TaskNodeContextMenu } from "./TaskNodeContextMenu";
import type { InternalStatus } from "@/types/status";
import type { Task } from "@/types/task";
import type { TaskNodeData } from "./TaskNode";

// ============================================================================
// Types
// ============================================================================

/**
 * Compact node uses the same data type as TaskNode for consistency
 */
export type TaskNodeCompactType = Node<TaskNodeData, "taskCompact">;


// ============================================================================
// Helper Functions
// ============================================================================

/**
 * Abbreviate text to fit compact node
 * Uses COMPACT_TITLE_MAX_CHARS from nodeStyles.ts
 */
function abbreviateTitle(text: string): string {
  if (text.length <= COMPACT_TITLE_MAX_CHARS) return text;
  // Try to break at word boundary
  const truncated = text.slice(0, COMPACT_TITLE_MAX_CHARS);
  const lastSpace = truncated.lastIndexOf(" ");
  if (lastSpace > COMPACT_TITLE_MAX_CHARS / 2) {
    return truncated.slice(0, lastSpace) + "…";
  }
  return truncated.slice(0, COMPACT_TITLE_MAX_CHARS - 1) + "…";
}

// ============================================================================
// Component
// ============================================================================

/**
 * Check if status should show activity dots
 */
function isActiveStatus(status: string): boolean {
  return status === "executing" || status === "re_executing" || status === "reviewing";
}

/**
 * Get activity dot color based on status
 * - Orange for executing states (matches accent)
 * - Blue for reviewing states (matches status-info)
 */
function getActivityDotColor(status: string): string {
  if (status === "reviewing") {
    return "var(--status-info)";
  }
  // executing, re_executing
  return "var(--accent-primary)";
}

function TaskNodeCompactComponent({ data, selected }: NodeProps<TaskNodeCompactType>) {
  const { label, taskId, internalStatus, priority, isCriticalPath, isHighlighted, isFocused, handlers, groupInfo } = data;
  const cardSurface = getBaseCardStyles(internalStatus as InternalStatus, false, false);
  const { compactNodeWidth } = useScaledNodeDimensions();
  const showActivityDots = isActiveStatus(internalStatus);
  const activityDotColor = getActivityDotColor(internalStatus);

  // Create a minimal task-like object for the context menu
  const minimalTask: Task = {
    id: taskId,
    projectId: "",
    category: "",
    title: label,
    description: null,
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

  // Node content
  const nodeContent = (
    <div
      className="relative"
      style={{ width: compactNodeWidth }}
      data-testid="task-node-compact"
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

      {/* Node content - compact version with glass morphism */}
      <div
        className={`
          relative rounded px-2 py-1.5
          transition-all duration-150 ease-out
          hover:shadow-md
          ${isCriticalPath && !selected && !isHighlighted && !isFocused ? "ring-1 ring-[var(--accent-border)]" : ""}
          ${isHighlighted ? "animate-pulse" : ""}
        `}
        style={{
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
        {/* Activity dots - top-right corner for active states */}
        {showActivityDots && (
          <div
            className="absolute top-1 right-1 flex gap-0.5"
            data-testid="activity-dots"
          >
            <span
              className="w-0.5 h-0.5 rounded-full"
              style={{
                backgroundColor: activityDotColor,
                animation: "bounce 1.4s ease-in-out 0s infinite",
              }}
            />
            <span
              className="w-0.5 h-0.5 rounded-full"
              style={{
                backgroundColor: activityDotColor,
                animation: "bounce 1.4s ease-in-out 0.2s infinite",
              }}
            />
            <span
              className="w-0.5 h-0.5 rounded-full"
              style={{
                backgroundColor: activityDotColor,
                animation: "bounce 1.4s ease-in-out 0.4s infinite",
              }}
            />
          </div>
        )}

        {/* Two-line title with line-clamp - no status badge */}
        <div
          className="font-medium leading-tight line-clamp-2"
          style={{
            fontSize: "0.75rem",
            color: "var(--text-primary)",
            lineHeight: 1.35,
          }}
          title={abbreviateTitle(label)}
        >
          {label}
        </div>
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

  // If no handlers provided, render without context menu
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
        onViewAgentChat: handleViewDetails,
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
 * Memoized TaskNodeCompact for React Flow performance
 */
export const TaskNodeCompact = memo(TaskNodeCompactComponent);
