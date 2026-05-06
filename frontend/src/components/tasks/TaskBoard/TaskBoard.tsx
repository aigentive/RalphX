/**
 * TaskBoard - Main kanban board component with drag-drop support
 *
 * Design: v29a Kanban
 * - Flat 48px toolbar
 * - Stable full-height column grid with 1px dividers
 * - Theme-specific card, empty-state, and status surfaces
 */

import { useState, useEffect, useMemo } from "react";
import {
  DndContext,
  DragOverlay,
  PointerSensor,
  useSensor,
  useSensors,
  type DragStartEvent,
  type DragEndEvent,
  type DragOverEvent,
} from "@dnd-kit/core";
import { useQuery, useQueryClient, type InfiniteData } from "@tanstack/react-query";
import { useEventBus } from "@/providers/EventProvider";
import { useTaskBoard } from "./hooks";
import { TaskBoardSkeleton } from "./TaskBoardSkeleton";
import { Column } from "./Column";
import { TaskCard } from "./TaskCard";
import { useUiStore } from "@/stores/uiStore";
import { usePlanStore, selectActiveExecutionPlanId } from "@/stores/planStore";
import { Toggle } from "@/components/ui/toggle";
import { EmptyState } from "@/components/ui/empty-state";
import { BarChart2, Database, FileText, Sparkles } from "lucide-react";
import { api } from "@/lib/tauri";
import { useTaskSearch } from "@/hooks/useTaskSearch";
import { TaskSearchBar } from "../TaskSearchBar";
import { EmptySearchState } from "../EmptySearchState";
import { PlanSelectorInline } from "@/components/plan/PlanSelectorInline";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { ProjectStatsCard } from "@/components/project/ProjectStatsCard";
import type { SelectionSource } from "@/api/plan";
import { infiniteTaskKeys } from "@/hooks/useInfiniteTasksQuery";
import { defaultWorkflow, type WorkflowColumn } from "@/types/workflow";
import type { Task, TaskListResponse, InternalStatus } from "@/types/task";
import { useColumnTaskCounts } from "./useColumnTaskCounts";
import { useColumnCollapse } from "./useColumnCollapse";

/**
 * Get all statuses for a column from its groups, or fallback to mapsTo
 */
function getColumnStatuses(col: WorkflowColumn): InternalStatus[] {
  if (col.groups && col.groups.length > 0) {
    const allStatuses = new Set<InternalStatus>();
    col.groups.forEach((group) => {
      group.statuses.forEach((status) => allStatuses.add(status));
    });
    return Array.from(allStatuses);
  }
  return [col.mapsTo];
}

export interface TaskBoardProps {
  projectId: string;
  /** Optional ideation session ID to filter tasks by plan */
  ideationSessionId?: string | null;
  /** Optional host-owned task selection handler for embedded task surfaces. */
  onTaskSelect?: (taskId: string) => void;
  /** Opens the global plan quick switcher with source attribution */
  onOpenPlanQuickSwitcher?: (source: SelectionSource) => void;
  /** When `true` (compact host like a task tab / chat-panel split), expanded
   * columns redistribute via `1fr` to fill the constrained width. When `false`
   * (full kanban screen), expanded columns keep a fixed width — collapsing one
   * just shifts the rest left and leaves empty space on the right. */
  fillWidth?: boolean;
}

export function TaskBoard({
  projectId,
  ideationSessionId: ideationSessionIdProp,
  onTaskSelect,
  onOpenPlanQuickSwitcher,
  fillWidth: fillWidthProp,
}: TaskBoardProps) {
  const queryClient = useQueryClient();
  const eventBus = useEventBus();
  const activePlanId = usePlanStore((s) => s.activePlanByProject[projectId] ?? null);
  const activePlanLoaded = usePlanStore(
    (s) => s.activePlanLoadedByProject[projectId] ?? false
  );
  // Get active execution plan ID for filtering (mutually exclusive with ideationSessionId)
  const activeExecutionPlanId = usePlanStore(selectActiveExecutionPlanId(projectId));
  // Use prop if provided, otherwise fall back to active plan from store.
  // When an executionPlanId is active, use null for ideationSessionId (exclusive filters).
  const ideationSessionId = ideationSessionIdProp ?? (activeExecutionPlanId ? null : activePlanId);
  // In Agents context (ideationSessionIdProp provided), don't use global execution plan — filter by session only.
  const effectiveExecutionPlanId = ideationSessionIdProp ? null : activeExecutionPlanId;

  // Load active plan from backend on mount or project change
  useEffect(() => {
    usePlanStore.getState().loadActivePlan(projectId);
  }, [projectId]);

  const { columns, onDragEnd, isLoading, error } = useTaskBoard(projectId, ideationSessionId, effectiveExecutionPlanId);
  const [activeTask, setActiveTask] = useState<Task | null>(null);
  const [overColumnId, setOverColumnId] = useState<string | null>(null);
  const [movingTaskId, setMovingTaskId] = useState<string | null>(null);
  const openModal = useUiStore((s) => s.openModal);
  const showArchived = useUiStore((s) => s.showArchived);
  const setShowArchived = useUiStore((s) => s.setShowArchived);
  // Default: full kanban screen → keep column widths fixed when one collapses
  // (empty space appears on the right). When a task tab is open the host
  // (or this fallback) flips to fillWidth, redistributing through `1fr`.
  const selectedTaskId = useUiStore((s) => s.selectedTaskId);
  const fillWidth = fillWidthProp ?? selectedTaskId != null;
  const showMergeTasks = true;
  const [isStatsOpen, setIsStatsOpen] = useState(false);
  const boardSearchQuery = useUiStore((s) => s.boardSearchQuery);
  const setBoardSearchQuery = useUiStore((s) => s.setBoardSearchQuery);

  // Column collapse: reactive task counts and auto-collapse/expand logic
  const taskCounts = useColumnTaskCounts(
    defaultWorkflow.columns,
    projectId,
    showArchived,
    ideationSessionId,
    showMergeTasks,
    effectiveExecutionPlanId,
  );
  const { isCollapsed, toggleCollapse, expandColumn } = useColumnCollapse(
    defaultWorkflow.columns,
    taskCounts,
    ideationSessionId,
  );

  // Fetch archived count to show/hide the toggle
  const { data: archivedCount = 0 } = useQuery({
    queryKey: ["archived-count", projectId, ideationSessionId],
    queryFn: () => api.tasks.getArchivedCount(projectId, ideationSessionId),
  });

  // Search functionality
  const {
    data: searchResults = [],
    isLoading: isSearchLoading,
  } = useTaskSearch({
    projectId,
    query: boardSearchQuery,
    includeArchived: showArchived,
    ideationSessionId,
  });

  // Check if search is active
  const isSearchActive = !!boardSearchQuery && boardSearchQuery.length >= 2;

  // When search is active, group search results by column
  const searchTasksByColumn = useMemo(() => {
    if (!isSearchActive) {
      return new Map<string, Task[]>();
    }

    // Map to column IDs
    const tasksByColumn = new Map<string, Task[]>();
    columns.forEach((column) => {
      // Keep search mapping aligned with rendering logic:
      // rendering uses groups resolved from defaultWorkflow by column.id.
      const workflowColumn = defaultWorkflow.columns.find((c) => c.id === column.id);
      const statusSource = workflowColumn ?? column;
      const columnStatuses = new Set(getColumnStatuses(statusSource));
      const tasks = searchResults.filter((task) =>
        columnStatuses.has(task.internalStatus)
      );
      if (tasks.length > 0) {
        tasksByColumn.set(column.id, tasks);
      }
    });

    return tasksByColumn;
  }, [columns, isSearchActive, searchResults]);

  // During search, filter columns to only show those with matches
  const displayColumns = useMemo(() => {
    if (!isSearchActive) {
      return columns;
    }
    // Only show columns with search results
    return columns.filter((col) => searchTasksByColumn.has(col.id));
  }, [columns, isSearchActive, searchTasksByColumn]);

  // Force-expand collapsed columns that have search results when search is active
  useEffect(() => {
    if (!isSearchActive) return;
    for (const [columnId] of searchTasksByColumn) {
      if (isCollapsed(columnId)) {
        expandColumn(columnId);
      }
    }
  }, [isSearchActive, searchTasksByColumn, isCollapsed, expandColumn]);

  // Clear movingTaskId after a short delay to allow optimistic update to settle
  useEffect(() => {
    if (!movingTaskId) return;
    const timeoutId = setTimeout(() => {
      setMovingTaskId(null);
    }, 100);
    return () => clearTimeout(timeoutId);
  }, [movingTaskId]);

  // Keyboard shortcuts: Cmd+N for new task, Cmd+F focuses search input, Escape clears search
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      // Guard: ignore if user is typing in an input/textarea
      const activeElement = document.activeElement;
      if (
        activeElement instanceof HTMLInputElement ||
        activeElement instanceof HTMLTextAreaElement ||
        activeElement?.hasAttribute('contenteditable')
      ) {
        return;
      }

      // Cmd+N / Ctrl+N: Open task creation modal
      if ((e.metaKey || e.ctrlKey) && e.key === 'n') {
        e.preventDefault();
        openModal('task-create', { projectId });
      }

      // Cmd+F / Ctrl+F: browser-level find should be disabled in board context
      if ((e.metaKey || e.ctrlKey) && e.key === 'f') {
        e.preventDefault();
      }

      // Escape: clear active search query
      if (e.key === 'Escape' && boardSearchQuery) {
        setBoardSearchQuery(null);
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [boardSearchQuery, setBoardSearchQuery, openModal, projectId]);

  // Listen for archive/restore/delete events for real-time updates
  useEffect(() => {
    const unsubscribers: (() => void)[] = [];

    // Listen for task:archived events
    const unsubArchived = eventBus.subscribe<{ taskId: string; projectId: string }>(
      'task:archived',
      (payload) => {
        // Only invalidate if the event is for the current project
        if (payload.projectId === projectId) {
          // Invalidate infinite task queries for all columns
          queryClient.invalidateQueries({
            queryKey: infiniteTaskKeys.all,
          });
          // Invalidate archived count
          queryClient.invalidateQueries({
            queryKey: ['archived-count', projectId, ideationSessionId],
          });
        }
      }
    );
    unsubscribers.push(unsubArchived);

    // Listen for task:restored events
    const unsubRestored = eventBus.subscribe<{ taskId: string; projectId: string }>(
      'task:restored',
      (payload) => {
        if (payload.projectId === projectId) {
          queryClient.invalidateQueries({
            queryKey: infiniteTaskKeys.all,
          });
          queryClient.invalidateQueries({
            queryKey: ['archived-count', projectId, ideationSessionId],
          });
        }
      }
    );
    unsubscribers.push(unsubRestored);

    return () => {
      unsubscribers.forEach((unsub) => unsub());
    };
  }, [projectId, ideationSessionId, queryClient, eventBus]);

  // Distance-based activation - drag starts after moving 8px
  const sensors = useSensors(
    useSensor(PointerSensor, {
      activationConstraint: {
        distance: 8,
      },
    })
  );

  // Task selection is now handled by TaskCard directly via setSelectedTaskId
  // which shows the TaskDetailOverlay in the split layout

  if (isLoading) {
    return <TaskBoardSkeleton />;
  }

  if (error) {
    return (
      <div
        data-testid="task-board-error"
        className="p-4 rounded-lg"
        style={{
          backgroundColor: "var(--bg-surface)",
          color: "var(--status-error)",
        }}
      >
        Error: {error.message || String(error)}
      </div>
    );
  }

  const handleDragStart = (event: DragStartEvent) => {
    const taskId = String(event.active.id);

    // Search for the task in the query cache (similar to onDragEnd in hooks.ts)
    let foundTask: Task | null = null;
    for (const col of columns) {
      const key = infiniteTaskKeys.list({
        projectId,
        statuses: getColumnStatuses(col),
        includeArchived: showArchived,
        ideationSessionId,
        executionPlanId: effectiveExecutionPlanId,
      });
      const data = queryClient.getQueryData<InfiniteData<TaskListResponse>>(key);
      if (data?.pages) {
        for (const page of data.pages) {
          const task = page.tasks.find((t: Task) => t.id === taskId);
          if (task) {
            foundTask = task;
            break;
          }
        }
      }
      if (foundTask) break;
    }

    setActiveTask(foundTask);
  };

  const handleDragOver = (event: DragOverEvent) => {
    setOverColumnId(event.over?.id.toString() || null);
  };

  const handleDragEnd = (event: DragEndEvent) => {
    const taskId = String(event.active.id);

    // Find the source column for this task
    const sourceColumn = columns.find((col) => {
      const key = infiniteTaskKeys.list({
        projectId,
        statuses: getColumnStatuses(col),
        includeArchived: showArchived,
        ideationSessionId,
        executionPlanId: effectiveExecutionPlanId,
      });
      const data = queryClient.getQueryData<InfiniteData<TaskListResponse>>(key);
      return data?.pages?.some((page) => page.tasks.some((t: Task) => t.id === taskId));
    });

    // Only hide the task if dropping on a DIFFERENT column
    // (prevents card from disappearing when dropped on same column or outside)
    const targetColumnId = event.over?.id.toString();
    if (targetColumnId && sourceColumn && targetColumnId !== sourceColumn.id) {
      setMovingTaskId(taskId);
    }

    // Auto-expand target column if it was collapsed
    if (targetColumnId && isCollapsed(targetColumnId)) {
      expandColumn(targetColumnId);
    }

    // Trigger optimistic update FIRST (onMutate is synchronous)
    onDragEnd(event);
    setActiveTask(null);
    setOverColumnId(null);
  };

  const handleDragCancel = () => {
    setActiveTask(null);
    setOverColumnId(null);
  };

  // Locked columns that can't receive drops
  const lockedColumns = ["in_progress", "in_review", "done"];

  // Check if we should show empty state
  const hasSearchResults = searchResults.length > 0;
  const showEmptyState = isSearchActive && !hasSearchResults && !isSearchLoading;
  const showNoPlanState = !ideationSessionIdProp && activePlanLoaded && !activePlanId && !isSearchActive;

  return (
    <DndContext
      sensors={sensors}
      onDragStart={handleDragStart}
      onDragOver={handleDragOver}
      onDragEnd={handleDragEnd}
      onDragCancel={handleDragCancel}
    >
      <div
        data-testid="task-board-root"
        className="flex h-full flex-col overflow-hidden"
        style={{ backgroundColor: "var(--app-content-bg)" }}
      >
        {/* v29a flat Kanban toolbar */}
        <div
          data-testid="kanban-toolbar"
          className="flex items-center gap-3 px-4"
          style={{
            minHeight: "48px",
            height: "48px",
            backgroundColor: "var(--kanban-toolbar-bg)",
            borderBottomColor: "var(--kanban-toolbar-border, #2E2E36)",
            borderBottomStyle: "solid",
            borderBottomWidth: "1px",
          }}
        >
          {/* Search Bar is always visible on Kanban */}
          <div className="w-[280px] shrink-0">
            <TaskSearchBar
              value={boardSearchQuery || ''}
              onChange={setBoardSearchQuery}
              onClose={() => {
                setBoardSearchQuery(null);
              }}
              resultCount={searchResults.length}
              isSearching={isSearchLoading}
            />
          </div>

          {/* Active plan selector in header row — hidden in Agents context (session-scoped) */}
          {!ideationSessionIdProp && (
            <PlanSelectorInline
              projectId={projectId}
              source="kanban_inline"
              onOpenPalette={(source) => onOpenPlanQuickSwitcher?.(source)}
            />
          )}

          {/* Archived tab - v29a keeps this in the subbar; merge tasks stay in Done groups. */}
          <Toggle
            pressed={showArchived}
            onPressedChange={setShowArchived}
            aria-label="Toggle show archived tasks"
            className="h-[30px] max-w-[380px] gap-2 rounded-[6px] px-2.5 text-[0.7812rem] font-medium transition-colors"
            style={{
              backgroundColor: showArchived ? "var(--bg-hover)" : "transparent",
              borderColor: showArchived ? "var(--border-default)" : "transparent",
              borderStyle: "solid",
              borderWidth: "1px",
              color: showArchived ? "var(--text-primary)" : "var(--text-muted)",
            }}
          >
            <Database className="h-[13px] w-[13px] shrink-0" />
            <span className="truncate">Archived</span>
            <span
              className="shrink-0 text-[0.6875rem]"
              style={{
                color: showArchived ? "var(--text-secondary)" : "var(--text-muted)",
                fontFamily:
                  "var(--font-mono, ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace)",
              }}
            >
              ({archivedCount})
            </span>
          </Toggle>

          {/* Project stats popover */}
          <Popover open={isStatsOpen} onOpenChange={setIsStatsOpen}>
            <PopoverTrigger asChild>
              <button
                className="ml-auto flex h-[30px] w-[30px] items-center justify-center rounded-md text-muted-foreground hover:text-foreground hover:bg-[var(--bg-hover)] transition-colors"
                aria-label="Project stats"
                style={{ border: "1px solid transparent" }}
              >
                <BarChart2 className="w-4 h-4" />
              </button>
            </PopoverTrigger>
            <PopoverContent align="end" className="w-96 p-0 border-[var(--border-subtle)] bg-transparent shadow-xl">
              <ProjectStatsCard projectId={projectId} />
            </PopoverContent>
          </Popover>
        </div>

        {/* TaskBoard container - v29a grid with 1px dividers */}
        <div
          data-testid="task-board"
          className="task-board relative flex-1 overflow-x-auto"
          style={{
            display: showNoPlanState || showEmptyState ? "flex" : "grid",
            gridTemplateColumns:
              showNoPlanState || showEmptyState
                ? undefined
                : (() => {
                    const anyCollapsed = displayColumns.some(
                      (c) => !activeTask && isCollapsed(c.id)
                    );
                    // Full-width split when nothing is collapsed (or in compact
                    // host) so columns fill the viewport via `1fr`. Once any
                    // column collapses on the bare kanban screen, freeze
                    // expanded columns to a fixed width — collapsing just
                    // shifts the rest left and leaves space on the right.
                    const useFixedWidths = !fillWidth && anyCollapsed;
                    return displayColumns
                      .map((column) =>
                        !activeTask && isCollapsed(column.id)
                          ? "128px"
                          : useFixedWidths
                            ? "320px"
                            : fillWidth
                              ? "minmax(0, 1fr)"
                              : "minmax(220px, 1fr)"
                      )
                      .join(" ");
                  })(),
            justifyContent: "start",
            gridAutoRows: "1fr",
            gap: showNoPlanState || showEmptyState ? undefined : "1px",
            alignItems: "stretch",
            backgroundColor: showNoPlanState || showEmptyState
              ? "var(--app-content-bg)"
              : "var(--kanban-board-divider)",
            minHeight: 0,
            scrollSnapType: "x proximity",
            scrollPaddingLeft: "0px",
            scrollPaddingRight: "0px",
            WebkitOverflowScrolling: "touch",
          }}
        >
          {/* Show no plan state when no active plan is selected */}
          {showNoPlanState ? (
            <div className="flex-1 flex items-center justify-center">
              <EmptyState
                iconBleed
                icon={
                  <div className="relative h-24 w-32">
                    <div
                      className="absolute left-2 top-5 h-14 w-20 rounded-xl"
                      style={{
                        background: "var(--overlay-faint)",
                        border: "1px solid var(--overlay-moderate)",
                      }}
                    />
                    <div
                      className="absolute right-2 top-5 h-14 w-20 rounded-xl"
                      style={{
                        background: "var(--overlay-faint)",
                        border: "1px solid var(--overlay-moderate)",
                      }}
                    />
                    <div
                      className="absolute left-1/2 top-1 -translate-x-1/2 h-20 w-24 rounded-2xl flex items-center justify-center"
                      style={{
                        background:
                          "linear-gradient(160deg, color-mix(in srgb, var(--accent-primary) 18%, transparent), color-mix(in srgb, var(--status-warning) 10%, transparent))",
                        border: "1px solid var(--accent-border)",
                        boxShadow: "0 12px 30px color-mix(in srgb, var(--accent-primary) 18%, transparent)",
                      }}
                    >
                      <FileText className="h-8 w-8" style={{ color: "var(--accent-primary)" }} />
                    </div>
                    <div className="absolute right-1 top-0">
                      <Sparkles className="h-4 w-4" style={{ color: "var(--status-warning)" }} />
                    </div>
                  </div>
                }
                title="No plan selected"
                description="Select a plan to view work on the Kanban board."
                action={
                  <div className="flex flex-col items-center gap-2">
                    <PlanSelectorInline
                      projectId={projectId}
                      source="kanban_inline"
                      onOpenPalette={(source) => onOpenPlanQuickSwitcher?.(source)}
                    />
                    <p className="text-xs text-[var(--text-muted)]">or press Cmd+Shift+P</p>
                  </div>
                }
                className="max-w-md"
              />
            </div>
          ) : /* Show empty search state when search has no results */
          showEmptyState ? (
            <div className="flex-1 flex items-center justify-center">
              <EmptySearchState
                searchQuery={boardSearchQuery || ''}
                onCreateTask={() => {
                  openModal('task-create', {
                    projectId,
                    defaultTitle: boardSearchQuery || undefined,
                  });
                }}
                onClearSearch={() => {
                  setBoardSearchQuery(null);
                }}
                showArchived={showArchived}
              />
            </div>
          ) : (
            <>
              {displayColumns.map((column, index) => {
                // In search mode, provide search results to column
                const searchTasks = isSearchActive
                  ? searchTasksByColumn.get(column.id)
                  : undefined;
                const matchCount = isSearchActive
                  ? searchTasks?.length
                  : undefined;

                // Look up groups from the default workflow for this column
                const workflowColumn = defaultWorkflow.columns.find(
                  (c) => c.id === column.id
                );
                const groups = workflowColumn?.groups;

                // Freeze collapse state during active drag (don't collapse/expand mid-drag)
                const columnCollapsed = !activeTask && isCollapsed(column.id);

                return (
                  <Column
                    key={column.id}
                    column={column}
                    projectId={projectId}
                    showArchived={showArchived}
                    showMergeTasks={showMergeTasks}
                    isOver={overColumnId === column.id}
                    isInvalid={
                      overColumnId === column.id && lockedColumns.includes(column.id)
                    }
                    hiddenTaskId={movingTaskId}
                    searchTasks={searchTasks}
                    matchCount={matchCount}
                    {...(groups && { groups })}
                    isLast={index === displayColumns.length - 1}
                    ideationSessionId={ideationSessionId}
                    executionPlanId={effectiveExecutionPlanId}
                    isCollapsed={columnCollapsed}
                    onToggleCollapse={() => toggleCollapse(column.id)}
                    fillWidth={fillWidth}
                    {...(onTaskSelect !== undefined && { onTaskSelect })}
                  />
                );
              })}
            </>
          )}
        </div>
        {/* Drag overlay with premium floating appearance */}
        <DragOverlay
          dropAnimation={{
            duration: 200,
            easing: "cubic-bezier(0.34, 1.56, 0.64, 1)",
          }}
        >
          {activeTask && <TaskCard task={activeTask} isDragging />}
        </DragOverlay>
      </div>
    </DndContext>
  );
}
