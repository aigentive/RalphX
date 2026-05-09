/* eslint-disable react-refresh/only-export-components */
/**
 * PlanGroup.tsx - Visual region component for plan groups in the Task Graph
 *
 * Renders as a React Flow group node that visually contains all task nodes
 * belonging to the same plan. Uses subtle background and rounded border
 * with PlanGroupHeader at the top.
 *
 * This component is used as a custom node type in React Flow.
 */

import { memo } from "react";
import type { NodeProps, Node } from "@xyflow/react";
import { Handle, Position } from "@xyflow/react";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import { GroupContextMenuItems } from "@/components/tasks/GroupContextMenuItems";
import { useConfirmation } from "@/hooks/useConfirmation";
import { PlanGroupHeader } from "./PlanGroupHeader";
import { UNGROUPED_PLAN_ID } from "./tierGroupUtils";
import type { StatusSummary } from "@/api/task-graph.types";
import { cn } from "@/lib/utils";
import { HEADER_HEIGHT } from "./groupUtils";
import { getPlanGroupNodeId } from "./groupTypes";

// ============================================================================
// Types
// ============================================================================

/**
 * Data shape for PlanGroup node
 */
export interface PlanGroupData extends Record<string, unknown> {
  /** Plan artifact ID for this group */
  planArtifactId: string;
  /** Session ID for navigation */
  sessionId: string;
  /** Session/plan title to display */
  sessionTitle: string | null;
  /** Task IDs contained in this group */
  taskIds: string[];
  /** Status summary with counts by category */
  statusSummary: StatusSummary;
  /** Whether the group is collapsed */
  isCollapsed: boolean;
  /** Width of the group region */
  width: number;
  /** Height of the group region */
  height: number;
  /** Tier group IDs within this plan (if any) */
  tierGroupIds?: string[];
  /** Whether any tiers are collapsed */
  anyTierCollapsed?: boolean;
  /** Whether all tiers are collapsed */
  allTiersCollapsed?: boolean;
  /** Toggle all tiers expand/collapse */
  onToggleAllTiers?: (planArtifactId: string, action: "expand" | "collapse") => void;
  /** Callback to toggle collapse state */
  onToggleCollapse?: (planArtifactId: string) => void;
  /** Project ID for API calls (feature branch settings) */
  projectId?: string;
  /** Selection state driven by graph selection */
  isSelected?: boolean;
  /** Navigate to a specific task (merge task link) */
  onNavigateToTask?: (taskId: string) => void;
  /** Cancel all tasks in this group (non-destructive alternative) */
  onCancelAll?: (sessionId: string) => void;
}

export type PlanGroupNode = Node<PlanGroupData, "planGroup">;

export interface PlanGroupProps extends NodeProps<PlanGroupNode> {
  /** Callback when collapse state should change */
  onToggleCollapse?: (planArtifactId: string) => void;
  /** Callback when context menu should open */
  onContextMenu?: (planArtifactId: string) => void;
  /** Callback to navigate to planning session */
  onNavigateToSession?: (sessionId: string) => void;
}

// ============================================================================
// Component
// ============================================================================

/**
 * PlanGroup - Visual region for tasks from the same plan
 *
 * Used as a React Flow custom node with type "planGroup".
 * Contains:
 * - Subtle background with rounded border
 * - PlanGroupHeader with title, progress, and status badges
 * - Empty content area (task nodes are positioned inside)
 *
 * @example
 * ```tsx
 * const nodeTypes = {
 *   planGroup: PlanGroup,
 *   task: TaskNode,
 * };
 *
 * <ReactFlow nodeTypes={nodeTypes} nodes={nodes} edges={edges} />
 * ```
 */
export const PlanGroup = memo(function PlanGroup({
  data,
  selected,
}: PlanGroupProps) {
  const {
    planArtifactId,
    sessionId,
    sessionTitle,
    taskIds,
    statusSummary,
    isCollapsed,
    width,
    height,
    tierGroupIds,
    anyTierCollapsed,
    allTiersCollapsed,
    onToggleAllTiers,
    onToggleCollapse,
    projectId,
    isSelected,
    onNavigateToTask,
    onCancelAll,
  } = data;
  const hasTierControls = Boolean(
    tierGroupIds && tierGroupIds.length > 0 && onToggleAllTiers
  );
  const tierControls = hasTierControls
    ? {
        tierGroupIds: tierGroupIds ?? [],
        anyTierCollapsed: Boolean(anyTierCollapsed),
        allTiersCollapsed: Boolean(allTiersCollapsed),
        onToggleAllTiers: onToggleAllTiers!,
      }
    : null;

  // When collapsed, show only the header (minimal padding)
  const displayHeight = isCollapsed ? HEADER_HEIGHT + 8 : height;

  const isGroupSelected = isSelected ?? selected;

  const isUncategorized = planArtifactId === UNGROUPED_PLAN_ID;
  const groupKind = isUncategorized ? "uncategorized" as const : "plan" as const;
  const groupLabel = sessionTitle ?? "Unnamed plan";
  const hasContextMenu = Boolean(onCancelAll && projectId);

  const { confirm, confirmationDialogProps, ConfirmationDialog } = useConfirmation();

  const groupContent = (
    <div
      className={cn(
        "rounded-lg overflow-hidden",
        "transition-all duration-200"
      )}
      style={{
        width,
        height: displayHeight,
        backgroundColor: "var(--bg-surface)",
        ...(isGroupSelected && {
          outline: "2px solid var(--accent-primary)",
          outlineOffset: "-2px",
        }),
      }}
      data-testid={`plan-group-${planArtifactId}`}
    >
      {/* Header */}
      <PlanGroupHeader
        planArtifactId={planArtifactId}
        {...(projectId ? { projectId } : {})}
        sessionTitle={sessionTitle}
        taskCount={taskIds.length}
        statusSummary={statusSummary}
        isCollapsed={isCollapsed}
        {...(tierControls ?? {})}
        onToggleCollapse={() => onToggleCollapse?.(planArtifactId)}
        {...(onNavigateToTask ? { onNavigateToTask } : {})}
      />

      {/* Content area - empty, task nodes are positioned inside by React Flow */}
      {!isCollapsed && (
        <div
          className="relative"
          style={{
            height: displayHeight - HEADER_HEIGHT,
          }}
        >
          {/* Task nodes render here via React Flow's coordinate system */}
        </div>
      )}

      {/* Invisible handles for inter-group edges */}
      <Handle
        type="target"
        position={Position.Top}
        className="!bg-transparent !border-0 !w-4 !h-1"
        style={{ top: 0, left: "50%", visibility: "hidden" }}
      />
      <Handle
        type="source"
        position={Position.Bottom}
        className="!bg-transparent !border-0 !w-4 !h-1"
        style={{ bottom: 0, left: "50%", visibility: "hidden" }}
      />
    </div>
  );

  if (!hasContextMenu) {
    return groupContent;
  }

  return (
    <>
      <ConfirmationDialog {...confirmationDialogProps} />
      <ContextMenu>
        <ContextMenuTrigger asChild>
          {groupContent}
        </ContextMenuTrigger>
        <ContextMenuContent data-testid={`plan-group-context-menu-${planArtifactId}`}>
          <GroupContextMenuItems
            groupLabel={groupLabel}
            groupKind={groupKind}
            taskCount={taskIds.length}
            projectId={projectId!}
            groupId={isUncategorized ? "" : sessionId}
            {...(onCancelAll && { onCancelAll: () => onCancelAll(sessionId) })}
            confirm={confirm}
          />
        </ContextMenuContent>
      </ContextMenu>
    </>

  );
});

// ============================================================================
// Factory Functions
// ============================================================================

/**
 * Create a PlanGroup node for React Flow
 *
 * @param planArtifactId - Unique ID for this plan group
 * @param sessionId - Session ID for navigation
 * @param sessionTitle - Title to display
 * @param taskIds - Task IDs in this group
 * @param statusSummary - Status counts
 * @param position - Top-left position of the group
 * @param width - Width of the group region
 * @param height - Height of the group region
 * @param isCollapsed - Whether the group starts collapsed
 * @param onToggleCollapse - Optional callback when collapse is toggled
 * @returns React Flow node object
 */
export function createPlanGroupNode(
  planArtifactId: string,
  sessionId: string,
  sessionTitle: string | null,
  taskIds: string[],
  statusSummary: StatusSummary,
  position: { x: number; y: number },
  width: number,
  height: number,
  isCollapsed = false,
  onToggleCollapse?: (planArtifactId: string) => void,
  tierGroupIds?: string[],
  anyTierCollapsed?: boolean,
  allTiersCollapsed?: boolean,
  onToggleAllTiers?: (planArtifactId: string, action: "expand" | "collapse") => void,
  projectId?: string,
  onNavigateToTask?: (taskId: string) => void,
  onCancelAll?: (sessionId: string) => void
): PlanGroupNode {
  return {
    id: getPlanGroupNodeId(planArtifactId),
    type: "planGroup",
    position,
    data: {
      planArtifactId,
      sessionId,
      sessionTitle,
      taskIds,
      statusSummary,
      isCollapsed,
      width,
      height,
      ...(tierGroupIds && tierGroupIds.length > 0 && onToggleAllTiers && {
        tierGroupIds,
        anyTierCollapsed: Boolean(anyTierCollapsed),
        allTiersCollapsed: Boolean(allTiersCollapsed),
        onToggleAllTiers,
      }),
      ...(onToggleCollapse && { onToggleCollapse }),
      ...(projectId && { projectId }),
      ...(onNavigateToTask && { onNavigateToTask }),
      ...(onCancelAll && { onCancelAll }),
    },
    // Group node properties
    style: {
      width,
      height: isCollapsed ? HEADER_HEIGHT + 8 : height,
    },
    // Make the group node non-draggable by default
    // (tasks inside can still be selected)
    draggable: false,
    selectable: true,
    // Ensure group renders behind task nodes
    zIndex: -2,
  };
}

/**
 * Node type key for registering PlanGroup with React Flow
 */
export const PLAN_GROUP_NODE_TYPE = "planGroup";
