/**
 * Column - Droppable column for the kanban board
 *
 * Design: v29a Kanban
 * - Stable full-height columns separated by 1px board dividers
 * - Flat card and empty-state surfaces with no glows
 * - Small uppercase section headers
 */

import { useDroppable, useDndContext } from "@dnd-kit/core";
import { ChevronLeft, Inbox, XCircle, Loader2 } from "lucide-react";
import { useRef, useEffect, useState, useMemo, useCallback } from "react";
import type { WorkflowColumnResponse } from "@/lib/api/workflows";
import type { StateGroup } from "@/types/workflow";
import type { InternalStatus } from "@/types/status";
import type { Task } from "@/types/task";
import { TaskCard, type TaskCardDisplayMode } from "./TaskCard";
import { ColumnGroup } from "./ColumnGroup";
import { useAgentGate } from "@/hooks/useAgentGate";
import { Skeleton } from "@/components/ui/skeleton";
import { InlineTaskAdd } from "../InlineTaskAdd";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import { GroupContextMenuItems } from "@/components/tasks/GroupContextMenuItems";
import { resolveGroupCleanupParams, type GroupInfo } from "@/lib/task-actions";
import { useConfirmation } from "@/hooks/useConfirmation";
import { useTaskMutation } from "@/hooks/useTaskMutation";
import {
  useInfiniteTasksQuery,
  flattenPages,
} from "@/hooks/useInfiniteTasksQuery";
import {
  getCollapsedGroups,
  saveCollapsedGroups,
  getGroupIcon,
} from "./Column.utils.tsx";
import { CollapsedQuickAdd } from "./CollapsedQuickAdd";
import { useProjectStats } from "@/hooks/useProjectStats";
import { formatMinutesHuman } from "@/lib/formatters";
import { TASK_BOARD_EXPANDED_COLUMN_MIN_WIDTH } from "./TaskBoard.layout";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";

interface ColumnProps {
  column: WorkflowColumnResponse;
  projectId: string;
  showArchived: boolean;
  showMergeTasks: boolean;
  isOver?: boolean;
  isInvalid?: boolean;
  onTaskSelect?: (taskId: string) => void;
  hiddenTaskId?: string | null;
  /** Optional search results to display instead of fetched tasks */
  searchTasks?: Task[] | undefined;
  /** Optional match count badge for search mode */
  matchCount?: number | undefined;
  /** Optional state groups for multi-state columns */
  groups?: StateGroup[];
  /** Hide right border for last column */
  isLast?: boolean;
  /** Optional ideation session ID to filter tasks by plan */
  ideationSessionId?: string | null | undefined;
  /** Optional execution plan ID to filter tasks (mutually exclusive with ideationSessionId) */
  executionPlanId?: string | null | undefined;
  /** Whether this column is collapsed */
  isCollapsed?: boolean;
  /** Callback to toggle collapse state */
  onToggleCollapse?: () => void;
  /** Preserved for host layout intent; expanded columns still keep their
   * readable minimum and let the board scroll horizontally when constrained. */
  fillWidth?: boolean;
  /** Card content density for tasks rendered in this column. */
  cardDisplayMode?: TaskCardDisplayMode;
  readOnly?: boolean;
}

function InvalidDropIcon() {
  return (
    <div data-testid="invalid-drop-icon" className="flex items-center justify-center" style={{ color: "var(--status-error)" }}>
      <XCircle className="w-5 h-5" />
    </div>
  );
}

function EmptyState({ compact = false }: { compact?: boolean }) {
  return (
    <div
      className={`flex flex-col items-center justify-center ${compact ? "gap-1.5 py-4 px-1" : "gap-2.5 px-3 pb-7 pt-9"}`}
      data-testid={compact ? "collapsed-empty-state" : undefined}
    >
      <div
        className="grid place-items-center rounded-lg"
        style={{
          width: compact ? "32px" : "36px",
          height: compact ? "32px" : "36px",
          backgroundColor: "var(--kanban-tray-bg)",
          color: "var(--kanban-empty-ink)",
        }}
        data-testid={compact ? "collapsed-empty-state-tray" : "empty-state-tray"}
      >
        <Inbox className={compact ? "w-4 h-4" : "w-[18px] h-[18px]"} />
      </div>
      <p
        style={{
          fontSize: compact ? "11px" : "12px",
          fontWeight: 500,
          color: "var(--kanban-empty-ink)",
        }}
        data-testid={compact ? "collapsed-empty-state-label" : "empty-state-label"}
      >
        No tasks
      </p>
    </div>
  );
}

function TaskSkeleton() {
  return (
    <div
      className="rounded-lg p-2.5 space-y-2"
      style={{
        backgroundColor: "var(--kanban-card-bg)",
        borderColor: "var(--kanban-card-border)",
        borderStyle: "solid",
        borderWidth: "1px",
      }}
    >
      <Skeleton className="h-3 w-3/4 rounded" style={{ backgroundColor: "var(--bg-hover)" }} />
      <Skeleton className="h-2.5 w-1/2 rounded" style={{ backgroundColor: "var(--kanban-progress-track)" }} />
    </div>
  );
}

function isCompletedDoneGroup(group: StateGroup): boolean {
  return (
    group.id === "completed" ||
    group.statuses.some((status) => status === "approved" || status === "merged")
  );
}

/**
 * Format average minutes into a compact human-readable string.
 * Examples: "2m", "7h 30m", "3d"
 */
function formatAvgCycleTime(avgMinutes: number): string {
  if (avgMinutes < 60) {
    return `${Math.round(avgMinutes)}m`;
  }
  const hours = avgMinutes / 60;
  if (hours < 24) {
    return formatMinutesHuman(avgMinutes);
  }
  const days = hours / 24;
  const wholeDays = Math.floor(days);
  const remainingHours = Math.round((days - wholeDays) * 24);
  if (remainingHours > 0) return `${wholeDays}d ${remainingHours}h`;
  return `${wholeDays}d`;
}

function formatColumnHeaderCount(
  taskCount: number,
  matchCount: number | undefined,
  avgCycleTime: number | null
): string {
  if (matchCount !== undefined) {
    return `(${taskCount} / ${matchCount})`;
  }

  if (avgCycleTime !== null) {
    return `(${taskCount} · ${formatAvgCycleTime(avgCycleTime)})`;
  }

  return `(${taskCount})`;
}

export function Column({ column, projectId, showArchived, showMergeTasks, isOver, isInvalid, onTaskSelect, hiddenTaskId, searchTasks, matchCount, groups, isLast = false, ideationSessionId, executionPlanId, isCollapsed = false, onToggleCollapse, cardDisplayMode = "default", readOnly = false }: ColumnProps) {
  const agentGate = useAgentGate();
  const { setNodeRef } = useDroppable({ id: column.id, disabled: readOnly });
  const sentinelRef = useRef<HTMLDivElement>(null);
  const { active } = useDndContext();
  const isDragging = active !== null;

  // Fetch project stats to show avg cycle time per column phase
  const { data: projectStats } = useProjectStats(projectId);

  // Find avg cycle time for the phase matching this column's mapsTo status
  const avgCycleTime = useMemo(() => {
    if (!projectStats) return null;
    const phase = projectStats.cycleTimeBreakdown.find(
      (p) => p.phase === column.mapsTo
    );
    if (!phase || phase.sampleSize === 0) return null;
    return phase.avgMinutes;
  }, [projectStats, column.mapsTo]);

  // Track collapsed state for groups (persisted to localStorage)
  const [collapsedGroups, setCollapsedGroups] = useState<Set<string>>(() =>
    getCollapsedGroups(column.id)
  );
  const [pendingQuickAddOpen, setPendingQuickAddOpen] = useState(0);

  // Handler for toggling group collapse state
  const handleToggleGroup = useCallback(
    (groupId: string) => {
      setCollapsedGroups((prev) => {
        const next = new Set(prev);
        if (next.has(groupId)) {
          next.delete(groupId);
        } else {
          next.add(groupId);
        }
        saveCollapsedGroups(column.id, next);
        return next;
      });
    },
    [column.id]
  );

  // Each column manages its own infinite query
  // Note: Backend only supports single status filter. For multi-state columns,
  // Gather all statuses from groups if they exist, otherwise use mapsTo
  const columnStatuses = useMemo(() => {
    if (groups && groups.length > 0) {
      // Collect all unique statuses from all groups
      const allStatuses = new Set<InternalStatus>();
      groups.forEach((group) => {
        group.statuses.forEach((status) => allStatuses.add(status));
      });
      return Array.from(allStatuses);
    }
    // No groups - use the primary mapsTo status
    return [column.mapsTo];
  }, [groups, column.mapsTo]);

  const {
    data,
    fetchNextPage,
    hasNextPage,
    isFetchingNextPage,
    isLoading,
  } = useInfiniteTasksQuery({
    projectId,
    statuses: columnStatuses,
    includeArchived: showArchived,
    ideationSessionId,
    executionPlanId,
  });

  // Use search tasks if provided (search mode), otherwise use fetched tasks
  const tasks = useMemo(() => {
    let result: Task[];
    if (searchTasks) {
      // In search mode, use provided search results
      result = searchTasks.sort((a, b) => a.priority - b.priority);
    } else {
      // In normal mode, flatten paginated data
      const flattened = flattenPages(data);
      result = flattened.sort((a, b) => a.priority - b.priority);
    }
    // Filter out merge tasks when toggle is off
    if (!showMergeTasks) {
      result = result.filter((t) => t.category !== "plan_merge");
    }
    return result;
  }, [data, searchTasks, showMergeTasks]);

  // Group tasks by their internal status when groups are defined
  const tasksByGroup = useMemo(() => {
    if (!groups || groups.length === 0) return null;

    const grouped = new Map<string, Task[]>();
    groups.forEach((group) => {
      grouped.set(group.id, []);
    });

    tasks.forEach((task) => {
      // Find which group this task belongs to based on its internalStatus
      const matchingGroup = groups.find((g) =>
        g.statuses.includes(task.internalStatus as InternalStatus)
      );
      if (matchingGroup) {
        const existing = grouped.get(matchingGroup.id) || [];
        grouped.set(matchingGroup.id, [...existing, task]);
      }
    });

    return grouped;
  }, [groups, tasks]);
  const unmappedTasks = useMemo(
    () =>
      groups
        ? tasks.filter(
            (task) =>
              !groups.some((group) =>
                group.statuses.includes(task.internalStatus as InternalStatus)
              )
          )
        : [],
    [groups, tasks]
  );

  // UX guard:
  // If Done has only completed tasks visible, force that subgroup open even if it was persisted collapsed.
  useEffect(() => {
    if (!groups || !tasksByGroup || column.mapsTo !== "approved") return;

    const nonEmptyGroups = groups.filter((group) => {
      const groupTasks = tasksByGroup.get(group.id) || [];
      return groupTasks.length > 0;
    });

    if (nonEmptyGroups.length !== 1) return;

    const onlyVisibleGroup = nonEmptyGroups[0];
    if (!onlyVisibleGroup || !isCompletedDoneGroup(onlyVisibleGroup)) return;

    setCollapsedGroups((prev) => {
      if (!prev.has(onlyVisibleGroup.id)) return prev;
      const next = new Set(prev);
      next.delete(onlyVisibleGroup.id);
      saveCollapsedGroups(column.id, next);
      return next;
    });
  }, [groups, tasksByGroup, column.mapsTo, column.id]);

  // Infinite scroll with IntersectionObserver
  // Re-run when isCollapsed changes so observer reconnects after expand
  useEffect(() => {
    const sentinel = sentinelRef.current;
    if (!sentinel) return;

    const observer = new IntersectionObserver(
      (entries) => {
        const entry = entries[0];
        // Trigger fetchNextPage when sentinel is visible AND there's more data AND not already fetching
        if (
          entry &&
          entry.isIntersecting &&
          hasNextPage &&
          !isFetchingNextPage
        ) {
          fetchNextPage();
        }
      },
      {
        root: null, // viewport
        rootMargin: "100px", // Load slightly before reaching the bottom
        threshold: 0.1,
      }
    );

    observer.observe(sentinel);

    return () => {
      observer.disconnect();
    };
  }, [hasNextPage, isFetchingNextPage, fetchNextPage, isCollapsed]);

  // Drop zone styles (v29a - flat, no ambient glow)
  const getDropZoneStyles = (): React.CSSProperties => {
    if (isOver && isInvalid) {
      return {
        backgroundColor: "var(--status-error-muted)",
        borderRadius: "6px",
      };
    }
    if (isOver) {
      return {
        backgroundColor: "var(--status-info-muted)",
        borderRadius: "6px",
      };
    }
    return {
      backgroundColor: "transparent",
      borderRadius: "0px",
    };
  };

  // Confirmation dialog for column context menu
  const { confirm, confirmationDialogProps, ConfirmationDialog } = useConfirmation();
  const { cancelTasksInGroupMutation, pauseTasksInGroupMutation, resumeTasksInGroupMutation, archiveTasksInGroupMutation } = useTaskMutation(projectId);

  // Handler for "Cancel all" group action
  // Note: For multi-state columns, this only cancels tasks matching the primary status (column.mapsTo)
  const handleCancelAll = useCallback(() => {
    const { groupKind, groupId } = resolveGroupCleanupParams("column", column.mapsTo);
    cancelTasksInGroupMutation.mutate({ groupKind, groupId, projectId });
  }, [column.mapsTo, projectId, cancelTasksInGroupMutation]);

  const handlePauseAll = useCallback(() => {
    const { groupKind, groupId } = resolveGroupCleanupParams("column", column.mapsTo);
    pauseTasksInGroupMutation.mutate({ groupKind, groupId, projectId });
  }, [column.mapsTo, projectId, pauseTasksInGroupMutation]);

  const handleResumeAll = useCallback(() => {
    // Resuming a group arms work — `ui:agent` (2.6-b). Pause/cancel/archive-all are
    // authority-reducing and stay available under the brakes boundary.
    if (agentGate.gated) return;
    const { groupKind, groupId } = resolveGroupCleanupParams("column", column.mapsTo);
    resumeTasksInGroupMutation.mutate({ groupKind, groupId, projectId });
  }, [agentGate.gated, column.mapsTo, projectId, resumeTasksInGroupMutation]);

  const handleArchiveAll = useCallback(() => {
    const { groupKind, groupId } = resolveGroupCleanupParams("column", column.mapsTo);
    archiveTasksInGroupMutation.mutate({ groupKind, groupId, projectId });
  }, [column.mapsTo, projectId, archiveTasksInGroupMutation]);

  // Group info for task-level context menus (shows column group actions)
  const columnGroupInfo: GroupInfo = useMemo(() => ({
    groupLabel: column.name,
    groupKind: "column" as const,
    taskCount: tasks.length,
    groupId: column.mapsTo,
    projectId,
    onCancelAll: handleCancelAll,
    onPauseAll: handlePauseAll,
    onResumeAll: handleResumeAll,
    onArchiveAll: handleArchiveAll,
  }), [column.name, column.mapsTo, tasks.length, projectId, handleCancelAll, handlePauseAll, handleResumeAll, handleArchiveAll]);

  // Determine if this column should show InlineTaskAdd
  // Always visible in draft/backlog columns (not during drag)
  const showInlineAdd =
    !readOnly &&
    !isDragging &&
    (column.id === "draft" || column.id === "backlog");
  const headerCountLabel = formatColumnHeaderCount(
    tasks.length,
    matchCount,
    avgCycleTime
  );

  // Keyboard handler for collapsed column (Enter/Space to expand)
  const handleCollapsedKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        onToggleCollapse?.();
      }
    },
    [onToggleCollapse]
  );

  const handleCollapsedQuickAdd = useCallback(() => {
    setPendingQuickAddOpen((value) => value + 1);
    onToggleCollapse?.();
  }, [onToggleCollapse]);

  const handleQuickAddConsumed = useCallback(() => {
    setPendingQuickAddOpen(0);
  }, []);

  // Collapsed rendering: compact rail with horizontal title, count badge, click-to-expand.
  if (isCollapsed) {
    return (
      <div
        ref={setNodeRef}
        data-testid={`column-${column.id}`}
        role="button"
        tabIndex={0}
        aria-expanded={false}
        aria-label={`${column.name} column, ${tasks.length} tasks. Click to expand`}
        onClick={onToggleCollapse}
        onKeyDown={handleCollapsedKeyDown}
        className="flex-shrink-0 flex flex-col items-start h-full cursor-pointer"
        style={{
          width: "128px",
          minWidth: "128px",
          maxWidth: "128px",
          paddingLeft: "10px",
          paddingRight: "10px",
          paddingTop: "8px",
          scrollSnapAlign: "start",
          transition: "width 200ms ease, min-width 200ms ease, max-width 200ms ease",
          ...(!isLast && {
            borderRightColor: "var(--kanban-board-divider)",
            borderRightStyle: "solid",
            borderRightWidth: "1px",
          }),
          // Drop zone highlight when dragging over collapsed column
          ...(isOver && !isInvalid && {
            backgroundColor: "var(--status-info-muted)",
            borderRadius: "6px",
          }),
          ...(isOver && isInvalid && {
            backgroundColor: "var(--status-error-muted)",
            borderRadius: "6px",
          }),
        }}
      >
        <div className="flex w-full items-center justify-between gap-2">
          <span
            className="select-none"
            style={{
              fontSize: "0.6875rem",
              fontWeight: 600,
              color: "var(--text-secondary)",
              textTransform: "uppercase",
              letterSpacing: "0.02em",
              whiteSpace: "nowrap",
              overflow: "hidden",
              textOverflow: "ellipsis",
              minWidth: 0,
            }}
          >
            {column.name}
          </span>
          <span
            style={{
              fontSize: "0.625rem",
              fontWeight: 500,
              color: "var(--text-muted)",
              fontVariantNumeric: "tabular-nums",
              flexShrink: 0,
            }}
          >
            {tasks.length}
            {matchCount !== undefined && ` / ${matchCount}`}
          </span>
        </div>

        {tasks.length === 0 && (
          <div className="mt-4 self-stretch">
            <EmptyState compact />
          </div>
        )}

        {/* Quick-add button for draft/backlog columns (hidden during drag) */}
        {!readOnly && !isDragging && (column.id === "draft" || column.id === "backlog") && (
          <CollapsedQuickAdd onActivate={handleCollapsedQuickAdd} />
        )}
      </div>
    );
  }

  return (
    <div
      data-testid={`column-${column.id}`}
      className="flex-shrink-0 flex flex-col h-full"
      style={{
        width: "100%",
        minWidth: `${TASK_BOARD_EXPANDED_COLUMN_MIN_WIDTH}px`,
        maxWidth: "none",
        scrollSnapAlign: "start",
        paddingLeft: 0,
        paddingRight: 0,
        backgroundColor: "var(--app-content-bg)",
        transition: "width 200ms ease, min-width 200ms ease, max-width 200ms ease",
        ...(!isLast && {
          borderRightColor: "var(--kanban-board-divider)",
          borderRightStyle: "solid",
          borderRightWidth: "1px",
        }),
      }}
    >
      {/* Column header - compact v29a section label */}
      <div
        data-testid="column-header"
        className="flex items-center gap-2"
        style={{ padding: "14px 12px 10px" }}
      >
        {/* Column title - small caps style like Finder */}
        <h3
          className="flex-1 m-0 truncate"
          style={{
            fontSize: "0.6562rem",
            fontWeight: 600,
            color: "var(--text-secondary)",
            textTransform: "uppercase",
            letterSpacing: "0.14em",
          }}
        >
          {column.name}
        </h3>

        {/* Count - simple, muted */}
        <span
          style={{
            fontSize: "0.6562rem",
            fontWeight: 500,
            color: "var(--text-subtle)",
            fontVariantNumeric: "tabular-nums",
            fontFamily: "var(--font-mono, ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace)",
          }}
          title={avgCycleTime !== null ? `Avg time in ${column.name}: ${formatAvgCycleTime(avgCycleTime)}` : undefined}
        >
          {headerCountLabel}
        </span>

        {/* Invalid drop indicator */}
        {isOver && isInvalid && <InvalidDropIcon />}

        {tasks.length === 0 && onToggleCollapse && (
          <TooltipProvider delayDuration={250}>
            <Tooltip>
              <TooltipTrigger asChild>
                <button
                  type="button"
                  aria-label={`Collapse ${column.name} column`}
                  onClick={onToggleCollapse}
                  className="flex h-5 w-5 items-center justify-center rounded"
                  style={{
                    color: "var(--text-muted)",
                    backgroundColor: "transparent",
                  }}
                >
                  <ChevronLeft className="h-3.5 w-3.5" />
                </button>
              </TooltipTrigger>
              <TooltipContent>Collapse column</TooltipContent>
            </Tooltip>
          </TooltipProvider>
        )}
      </div>

      {/* Drop zone with task list - clean, minimal */}
      <ContextMenu>
        <ContextMenuTrigger asChild>
          <div
            ref={setNodeRef}
            data-testid={`drop-zone-${column.id}`}
            className="flex-1 flex flex-col gap-2 overflow-y-auto transition-colors duration-150"
            style={{
              ...getDropZoneStyles(),
              padding: "4px 12px 16px",
            }}
          >
            {/* Show skeleton cards during initial load */}
            {isLoading ? (
              <>
                <TaskSkeleton />
                <TaskSkeleton />
                <TaskSkeleton />
              </>
            ) : tasks.length === 0 ? (
              <EmptyState />
            ) : groups && groups.length > 0 && tasksByGroup ? (
              /* Grouped rendering - render ColumnGroup for each group */
              <>
                {groups.map((group) => {
                  const groupTasks = tasksByGroup.get(group.id) || [];
                  // Only render groups that have tasks
                  if (groupTasks.length === 0) return null;

                  return (
                    <ColumnGroup
                      key={group.id}
                      label={group.label}
                      count={groupTasks.length}
                      icon={getGroupIcon(group.icon)}
                      {...(group.accentColor && { accentColor: group.accentColor })}
                      collapsed={collapsedGroups.has(group.id)}
                      onToggle={() => handleToggleGroup(group.id)}
                    >
                      {groupTasks.map((task) => (
                        <TaskCard
                          key={task.id}
                          task={task}
                          displayMode={cardDisplayMode}
                          isHidden={task.id === hiddenTaskId}
                          groupInfo={columnGroupInfo}
                          readOnly={readOnly}
                          {...(onTaskSelect !== undefined && { onSelect: onTaskSelect })}
                        />
                      ))}
                    </ColumnGroup>
                  );
                })}

                {unmappedTasks.length > 0 && (
                  <ColumnGroup
                    label="Unmapped status"
                    count={unmappedTasks.length}
                    collapsed={false}
                    onToggle={() => {}}
                  >
                    {unmappedTasks.map((task) => (
                      <TaskCard
                        key={task.id}
                        task={task}
                        displayMode={cardDisplayMode}
                        isHidden={task.id === hiddenTaskId}
                        readOnly
                        {...(onTaskSelect !== undefined && { onSelect: onTaskSelect })}
                      />
                    ))}
                  </ColumnGroup>
                )}

                {/* Sentinel element for infinite scroll */}
                <div ref={sentinelRef} className="h-1" aria-hidden="true" />

                {/* Loading spinner when fetching next page - simple */}
                {isFetchingNextPage && (
                  <div className="flex items-center justify-center py-3">
                    <Loader2 className="w-4 h-4 animate-spin" style={{ color: "var(--text-muted)" }} />
                  </div>
                )}
              </>
            ) : (
              /* Ungrouped rendering - render tasks directly */
              <>
                {tasks.map((task) => (
                  <TaskCard
                    key={task.id}
                    task={task}
                    displayMode={cardDisplayMode}
                    isHidden={task.id === hiddenTaskId}
                    groupInfo={columnGroupInfo}
                    readOnly={readOnly}
                    {...(onTaskSelect !== undefined && { onSelect: onTaskSelect })}
                  />
                ))}

                {/* Sentinel element for infinite scroll */}
                <div ref={sentinelRef} className="h-1" aria-hidden="true" />

                {/* Loading spinner when fetching next page - simple */}
                {isFetchingNextPage && (
                  <div className="flex items-center justify-center py-3">
                    <Loader2 className="w-4 h-4 animate-spin" style={{ color: "var(--text-muted)" }} />
                  </div>
                )}
              </>
            )}

            {/* Inline task add - always visible in draft/backlog columns (hidden during drag) */}
            {showInlineAdd && (
              <InlineTaskAdd
                projectId={projectId}
                columnId={column.id}
                ideationSessionId={ideationSessionId}
                executionPlanId={executionPlanId}
                {...(pendingQuickAddOpen > 0 && { autoExpandKey: pendingQuickAddOpen })}
                onAutoExpandConsumed={handleQuickAddConsumed}
              />
            )}
          </div>
        </ContextMenuTrigger>
        <ContextMenuContent>
          <GroupContextMenuItems
            groupLabel={column.name}
            groupKind="column"
            taskCount={tasks.length}
            projectId={projectId}
            groupId={column.mapsTo}
            {...(!readOnly && {
              onCancelAll: handleCancelAll,
              onPauseAll: handlePauseAll,
              onResumeAll: handleResumeAll,
              onArchiveAll: handleArchiveAll,
            })}
            confirm={confirm}
          />
        </ContextMenuContent>
        <ConfirmationDialog {...confirmationDialogProps} />
      </ContextMenu>
    </div>
  );
}
