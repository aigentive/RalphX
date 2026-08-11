/**
 * AgentsTaskDetailOverlay - Agents-owned inline task detail panel.
 *
 * It displays as an overlay inside the Agents artifact surface so Agents can
 * evolve task details independently from the embedded Kanban and Graph modes.
 *
 * Design spec: specs/design/refined-studio-patterns.md
 */

import React, { useCallback, useEffect, useState } from "react";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { ErrorBoundary } from "@/components/ErrorBoundary";
import { AgentsTaskDetailPanel } from "./AgentsTaskDetailPanel";
import { TaskEditForm } from "./TaskEditForm";
import { StatusDropdown } from "./StatusDropdown";
import { TaskHistoryDropdown } from "./TaskHistoryDropdown";
import { useQuery } from "@tanstack/react-query";
import { useTaskMutation } from "@/hooks/useTaskMutation";
import { useUiStore } from "@/stores/uiStore";
import { useTaskStore } from "@/stores/taskStore";
import { useTasks, taskKeys } from "@/hooks/useTasks";
import { api } from "@/lib/tauri";
import type { Task, InternalStatus } from "@/types/task";
import {
  Pencil,
  Archive,
  RotateCcw,
  Loader2,
  Lightbulb,
  History,
  ScrollText,
  ChevronLeft,
  Tag,
} from "lucide-react";
import { useIdeationStore } from "@/stores/ideationStore";
import { useCreateIdeationSession } from "@/hooks/useIdeation";
import { useConfirmation } from "@/hooks/useConfirmation";
import { navigateToIdeationSession } from "@/lib/navigation";
import { logger } from "@/lib/logger";
import { toast } from "sonner";
import { AuditTrailDialog } from "./AuditTrailDialog";
import { getTaskCategoryLabel } from "@/lib/task-category";
import { cn } from "@/lib/utils";
import type {
  TaskHistoryState,
  TaskRuntimeHistoryContextType,
} from "@/types/task-history";

// ============================================================================
// Priority Colors (Tahoe HSL palette)
// ============================================================================

const PRIORITY_COLORS: Record<number, { bg: string; text: string }> = {
  1: { bg: "var(--status-error)", text: "white" },
  2: { bg: "var(--accent-primary)", text: "white" },
  3: { bg: "var(--status-warning)", text: "var(--bg-surface)" },
  4: { bg: "var(--bg-hover)", text: "var(--text-secondary)" },
};

const DEFAULT_PRIORITY_COLOR = { bg: "var(--bg-hover)", text: "var(--text-secondary)" };

// ============================================================================
// Status Badge Configuration (Tahoe HSL palette)
// ============================================================================

const STATUS_CONFIG: Record<
  InternalStatus,
  { label: string; bg: string; text: string }
> = {
  backlog: {
    label: "Backlog",
    bg: "var(--bg-hover)",
    text: "var(--text-muted)",
  },
  ready: {
    label: "Ready",
    bg: "var(--status-info-muted)",
    text: "var(--status-info)",
  },
  blocked: {
    label: "Blocked",
    bg: "var(--status-warning-muted)",
    text: "var(--status-warning)",
  },
  executing: {
    label: "Executing",
    bg: "var(--accent-muted)",
    text: "var(--accent-primary)",
  },
  qa_refining: {
    label: "QA Refining",
    bg: "var(--accent-muted)",
    text: "var(--accent-primary)",
  },
  qa_testing: {
    label: "QA Testing",
    bg: "var(--accent-muted)",
    text: "var(--accent-primary)",
  },
  qa_passed: {
    label: "QA Passed",
    bg: "var(--status-success-muted)",
    text: "var(--status-success)",
  },
  qa_failed: {
    label: "QA Failed",
    bg: "var(--status-error-muted)",
    text: "var(--status-error)",
  },
  pending_review: {
    label: "Pending Review",
    bg: "var(--status-warning-muted)",
    text: "var(--status-warning)",
  },
  revision_needed: {
    label: "Revision Needed",
    bg: "var(--status-warning-muted)",
    text: "var(--status-warning)",
  },
  approved: {
    label: "Approved",
    bg: "var(--status-success-muted)",
    text: "var(--status-success)",
  },
  failed: {
    label: "Failed",
    bg: "var(--status-error-muted)",
    text: "var(--status-error)",
  },
  cancelled: {
    label: "Cancelled",
    bg: "var(--bg-hover)",
    text: "var(--text-muted)",
  },
  reviewing: {
    label: "AI Review in Progress",
    bg: "var(--status-info-muted)",
    text: "var(--status-info)",
  },
  review_passed: {
    label: "AI Review Passed",
    bg: "var(--status-success-muted)",
    text: "var(--status-success)",
  },
  escalated: {
    label: "Escalated",
    bg: "var(--status-warning-muted)",
    text: "var(--status-warning)",
  },
  re_executing: {
    label: "Re-executing",
    bg: "var(--accent-muted)",
    text: "var(--accent-primary)",
  },
  pending_merge: {
    label: "Pending Merge",
    bg: "var(--accent-muted)",
    text: "var(--accent-primary)",
  },
  merging: {
    label: "Merging",
    bg: "var(--accent-muted)",
    text: "var(--accent-primary)",
  },
  waiting_on_pr: {
    label: "Waiting on PR",
    bg: "var(--status-info-muted)",
    text: "var(--status-info)",
  },
  merge_incomplete: {
    label: "Merge Incomplete",
    bg: "var(--status-warning-muted)",
    text: "var(--status-warning)",
  },
  merge_conflict: {
    label: "Merge Conflict",
    bg: "var(--status-warning-muted)",
    text: "var(--status-warning)",
  },
  updating_plan_branch: {
    label: "Updating Plan Branch",
    bg: "var(--status-info-muted)",
    text: "var(--status-info)",
  },
  updating_task_branch: {
    label: "Updating Task Branch",
    bg: "var(--status-info-muted)",
    text: "var(--status-info)",
  },
  branch_update_blocked: {
    label: "Branch Update Blocked",
    bg: "var(--status-warning-muted)",
    text: "var(--status-warning)",
  },
  merged: {
    label: "Merged",
    bg: "var(--status-success-muted)",
    text: "var(--status-success)",
  },
  paused: {
    label: "Paused",
    bg: "var(--status-warning-muted)",
    text: "var(--status-warning)",
  },
  stopped: {
    label: "Stopped",
    bg: "var(--status-error-muted)",
    text: "var(--status-error)",
  },
};

// ============================================================================
// Sub-components
// ============================================================================

function PriorityBadge({ priority }: { priority: number }) {
  const colors = PRIORITY_COLORS[priority] ?? DEFAULT_PRIORITY_COLOR;
  return (
    <span
      data-testid="task-overlay-priority"
      className="inline-flex items-center gap-1.5 px-2 py-0.5 rounded-full text-[0.6875rem] font-semibold tracking-tight"
      style={{ backgroundColor: colors.bg, color: colors.text }}
      title={`Priority P${priority}`}
    >
      <span
        aria-hidden
        className="w-1.5 h-1.5 rounded-full bg-current opacity-70"
      />
      P{priority}
    </span>
  );
}

function CategoryBadge({ label }: { label: string }) {
  return (
    <span
      data-testid="task-overlay-category"
      className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-[0.6875rem] font-medium"
      style={{
        backgroundColor: "var(--overlay-faint)",
        color: "var(--text-secondary)",
      }}
    >
      <Tag className="w-3 h-3 opacity-70" />
      {label}
    </span>
  );
}

function StatusBadge({ status }: { status: InternalStatus }) {
  const config = STATUS_CONFIG[status];
  return (
    <Badge
      data-testid="task-overlay-status"
      data-status={status}
      className="rounded px-1.5 py-0.5 text-[0.625rem] font-medium border-0"
      style={{ backgroundColor: config.bg, color: config.text }}
    >
      {config.label}
    </Badge>
  );
}

interface HeaderIconButtonProps
  extends Omit<React.ComponentProps<typeof Button>, "children"> {
  tooltip: string;
  children: React.ReactNode;
}

function HeaderIconButton({
  tooltip,
  children,
  ...buttonProps
}: HeaderIconButtonProps) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          {...buttonProps}
          aria-label={buttonProps["aria-label"] ?? tooltip}
          style={{ color: "var(--text-secondary)", ...buttonProps.style }}
          className={`hover:bg-[var(--overlay-weak)] hover:text-[var(--text-primary)] ${buttonProps.className ?? ""}`}
        >
          {children}
        </Button>
      </TooltipTrigger>
      <TooltipContent>{tooltip}</TooltipContent>
    </Tooltip>
  );
}

// ============================================================================
// Main Component
// ============================================================================

export interface AgentsTaskDetailOverlayProps {
  projectId: string;
  /** Optional footer to render at the bottom of the overlay (e.g., ExecutionControlBar) */
  footer?: React.ReactNode;
  /** Center task details in a readable column when no adjacent chat is present. */
  constrainContent?: boolean;
  /** Optional host-owned selected task id for embedded task surfaces. */
  selectedTaskIdOverride?: string | null;
  /** Optional host-owned close handler for embedded task surfaces. */
  onCloseOverride?: () => void;
  /** When provided, renders a back row above the title (e.g., "Back to Kanban"). */
  backLabel?: string;
  /** Click handler for the back button. Defaults to onCloseOverride when not given. */
  onBack?: () => void;
  /** Focus the host Agents chat on a task runtime transcript. */
  onFocusTaskRuntime?: (
    taskId: string,
    contextType: TaskRuntimeHistoryContextType
  ) => void;
  readOnly?: boolean;
}

export function AgentsTaskDetailOverlay({
  projectId,
  footer,
  constrainContent = false,
  selectedTaskIdOverride,
  onCloseOverride,
  backLabel,
  onBack,
  onFocusTaskRuntime,
  readOnly = false,
}: AgentsTaskDetailOverlayProps) {
  const selectedTaskId = selectedTaskIdOverride ?? null;
  // History state from store - shared with the Agents chat surface
  const historyState = useUiStore((s) => s.taskHistoryState);
  const setHistoryState = useUiStore((s) => s.setTaskHistoryState);

  // Debug logging for history state
  logger.debug('[AgentsTaskDetailOverlay] History state from store:', historyState);

  // Ideation hooks
  const addSession = useIdeationStore((state) => state.addSession);
  const setActiveSession = useIdeationStore((state) => state.setActiveSession);
  const createSession = useCreateIdeationSession();

  // Try to get task from store first, fall back to fetching from API
  const taskFromStore = useTaskStore((state) =>
    selectedTaskId ? state.tasks[selectedTaskId] : undefined
  );

  // Fetch all tasks to ensure we have the latest data
  const { data: tasks = [] } = useTasks(projectId);

  // Find the task from the list query
  const taskFromList = tasks.find((t) => t.id === selectedTaskId);

  // Fallback: fetch the specific task by ID when not found in store or paginated list
  const { data: taskFromDetail } = useQuery<Task, Error>({
    queryKey: taskKeys.detail(selectedTaskId ?? ""),
    queryFn: () => api.tasks.get(selectedTaskId!),
    enabled: Boolean(selectedTaskId) && !taskFromStore && !taskFromList,
  });

  const task: Task | undefined = taskFromStore || taskFromList || taskFromDetail;

  const [isEditing, setIsEditing] = useState(false);
  const [showAuditTrail, setShowAuditTrail] = useState(false);

  const closeSelectedTask = useCallback(() => {
    if (onCloseOverride) {
      onCloseOverride();
      return;
    }
  }, [onCloseOverride]);

  const handleClose = useCallback(() => {
    closeSelectedTask();
    setIsEditing(false);
  }, [closeSelectedTask]);

  const handleHistoryStateSelect = useCallback(
    (state: TaskHistoryState | null) => {
      setHistoryState(state);
      if (state?.conversationId && state.contextType && selectedTaskId) {
        onFocusTaskRuntime?.(selectedTaskId, state.contextType);
      }
    },
    [onFocusTaskRuntime, selectedTaskId, setHistoryState]
  );

  // Derived values for history mode (historyState from store)
  const isHistoryMode = historyState !== null;
  const viewStatus = (historyState?.status as InternalStatus | undefined) ?? task?.internalStatus;
  const historyContextLabel =
    historyState?.contextType === "task_execution"
      ? "Execution"
      : historyState?.contextType === "review"
        ? "Review"
        : historyState?.contextType === "merge"
          ? "Merge"
          : historyState?.contextType === "branch_update"
            ? "Branch Update"
          : null;
  const historyStageLabel =
    historyContextLabel && historyState?.attemptIndex
      ? `${historyContextLabel} attempt ${historyState.attemptIndex}`
      : historyState
        ? STATUS_CONFIG[historyState.status]?.label ?? historyState.status
        : "";
  const historyHasConversation =
    historyState?.hasConversation ?? Boolean(historyState?.conversationId);
  const historyTranscriptMessage = historyHasConversation
    ? onFocusTaskRuntime
      ? "Main chat is showing this runtime transcript"
      : "Runtime transcript available"
    : "No runtime transcript recorded for this stage";
  const isEditMode = isEditing && !readOnly && !isHistoryMode;

  // Get mutations
  const {
    updateMutation,
    moveMutation,
    archiveMutation,
    restoreMutation,
    isArchiving,
    isRestoring,
  } = useTaskMutation(projectId);

  // Confirmation dialog for archive/restore
  const { confirm, confirmationDialogProps, ConfirmationDialog } = useConfirmation();

  // Close overlay on Escape key (exit edit mode first, then close overlay)
  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        if (isEditMode) {
          // If editing, just exit edit mode (go back to view)
          setIsEditing(false);
        } else {
          // If viewing, close the overlay
          closeSelectedTask();
        }
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [closeSelectedTask, isEditMode]);

  // Reset editing and history state when task changes
  useEffect(() => {
    setIsEditing(false);
    setHistoryState(null);
  }, [selectedTaskId, setHistoryState]);

  useEffect(() => {
    if (readOnly || isHistoryMode) {
      setIsEditing(false);
    }
  }, [isHistoryMode, readOnly]);

  // Handle backdrop click
  const handleBackdropClick = useCallback(
    (event: React.MouseEvent<HTMLDivElement>) => {
      // Only close if clicking the backdrop itself, not its children
      if (event.target === event.currentTarget) {
        handleClose();
      }
    },
    [handleClose]
  );

  // Handle edit save
  const handleSave = (updateData: Parameters<typeof updateMutation.mutate>[0]['input']) => {
    if (!task || readOnly) return;
    updateMutation.mutate(
      { taskId: task.id, input: updateData },
      {
        onSuccess: () => {
          setIsEditing(false);
        },
      }
    );
  };

  // Handle status change
  const handleStatusChange = (newStatus: string) => {
    if (!task) return;
    moveMutation.mutate({ taskId: task.id, toStatus: newStatus });
  };

  // Handle archive
  const handleArchive = async () => {
    if (!task) return;
    const confirmed = await confirm({
      title: "Archive this task?",
      description: "The task will be moved to the archive.",
      confirmText: "Archive",
    });
    if (!confirmed) return;
    archiveMutation.mutate(task.id, {
      onSuccess: () => {
        handleClose();
      },
    });
  };

  // Handle restore
  const handleRestore = async () => {
    if (!task) return;
    const confirmed = await confirm({
      title: "Restore this task?",
      description: "The task will be restored to the backlog.",
      confirmText: "Restore",
    });
    if (!confirmed) return;
    restoreMutation.mutate(task.id, {
      onSuccess: () => {
        handleClose();
      },
    });
  };

  // Handle start ideation
  const handleStartIdeation = async () => {
    if (!task) return;
    try {
      // Create session with seedTaskId
      const session = await createSession.mutateAsync({
        projectId: task.projectId,
        title: `Ideation: ${task.title}`,
        seedTaskId: task.id,
      });
      // Add session to store and set as active
      addSession(session);
      setActiveSession(session.id);
      // Close overlay and navigate to the linked plan surface.
      handleClose();
      navigateToIdeationSession(session.id);
    } catch (error) {
      console.error("Failed to start ideation:", error);
      toast.error("Failed to start ideation session");
    }
  };

  // Don't render if no task is selected
  if (!selectedTaskId || !task) {
    if (selectedTaskId && !task) {
      console.warn('[AgentsTaskDetailOverlay] Task not found for selectedTaskId:', selectedTaskId);
    }
    return null;
  }

  // Determine if task is editable
  const systemControlledStatuses: InternalStatus[] = [
    "executing",
    "qa_refining",
    "qa_testing",
    "qa_passed",
    "qa_failed",
    "pending_review",
    "revision_needed",
    "reviewing",
    "review_passed",
    "re_executing",
  ];

  const isArchived = !!task.archivedAt;
  const isManagedPlanMerge = task.category === "plan_merge";
  const isSystemControlled = isManagedPlanMerge || systemControlledStatuses.includes(task.internalStatus);
  const canEdit = !readOnly && !isHistoryMode && !isArchived && !isSystemControlled;
  const categoryLabel = getTaskCategoryLabel(task.category);
  // "Backlog" is the equivalent of "draft" - tasks that haven't started execution yet
  const isBacklog = task.internalStatus === "backlog";
  const contentFrameClassName = constrainContent
    ? "mx-auto w-full max-w-[1500px]"
    : "w-full";

  return (
    <>
      {/* Full-page container */}
      <div
        data-testid="task-overlay-backdrop"
        className="absolute inset-0 z-40 flex"
        style={{
          backgroundColor: "var(--bg-base)",
        }}
        onClick={handleBackdropClick}
      >
        {/* Content area - full width, no boxing */}
        <div
          data-testid="task-detail-overlay"
          data-task-id={task.id}
          className="flex-1 flex flex-col"
          onClick={(e) => e.stopPropagation()}
        >
          {/* Header - flat Tahoe styling */}
          <div
            className="px-6 pt-5 pb-4 shrink-0"
            style={{
              borderBottom: "1px solid var(--overlay-weak)",
            }}
          >
            {/* Back to source view (Kanban / Graph) */}
            {backLabel && (onBack || onCloseOverride) && (
              <button
                type="button"
                data-testid="task-overlay-back"
                onClick={onBack ?? onCloseOverride}
                className="mb-2 inline-flex items-center gap-1 -ml-1 px-1.5 py-0.5 rounded-md text-[0.75rem] font-medium transition-colors hover:bg-[var(--overlay-faint)]"
                style={{ color: "var(--text-secondary)" }}
              >
                <ChevronLeft className="w-3.5 h-3.5" />
                {backLabel}
              </button>
            )}
            {/* Archived Badge */}
            {isArchived && (
              <div
                data-testid="archived-badge"
                className="mb-3 px-2.5 py-1.5 rounded-lg flex items-center gap-2 w-fit"
                style={{
                  backgroundColor: "var(--accent-muted)",
                  border: "1px solid var(--accent-border)",
                }}
              >
                <Archive className="w-3.5 h-3.5" style={{ color: "var(--accent-primary)" }} />
                <span className="text-[0.75rem] font-medium" style={{ color: "var(--accent-primary)" }}>Archived</span>
              </div>
            )}
            <div className="pr-28">
              <h2
                data-testid="task-overlay-title"
                className="text-base font-semibold truncate"
                style={{
                  color: "var(--text-primary)",
                  letterSpacing: "-0.02em",
                  lineHeight: "1.3",
                }}
              >
                {task.title}
              </h2>
              <div className="flex flex-wrap items-center gap-1.5 mt-1.5">
                <PriorityBadge priority={task.priority} />
                <CategoryBadge label={categoryLabel} />
                {task.internalStatus !== "waiting_on_pr" && (
                  <StatusBadge status={task.internalStatus} />
                )}
              </div>
            </div>

            {/* Action buttons */}
            <TooltipProvider delayDuration={250}>
              <div className="absolute top-4 right-4 flex items-center gap-2">
                {/* StatusDropdown - only for user-controlled statuses */}
                {canEdit && (
                  <StatusDropdown
                    taskId={task.id}
                    currentStatus={task.internalStatus}
                    onTransition={handleStatusChange}
                    disabled={moveMutation.isPending}
                  />
                )}
                {/* Start Ideation button - only for backlog (draft) tasks */}
                {!readOnly && !isHistoryMode && isBacklog && (
                  <HeaderIconButton
                    variant="ghost"
                    size="icon-sm"
                    onClick={handleStartIdeation}
                    disabled={createSession.isPending}
                    data-testid="task-overlay-ideation-button"
                    aria-label="Start Ideation"
                    tooltip="Start ideation"
                  >
                    {createSession.isPending ? (
                      <Loader2 className="w-4 h-4 animate-spin" />
                    ) : (
                      <Lightbulb className="w-4 h-4" />
                    )}
                  </HeaderIconButton>
                )}
                {/* Edit button */}
                {canEdit && (
                  <HeaderIconButton
                    variant="ghost"
                    size="icon-sm"
                    onClick={() => setIsEditing(!isEditing)}
                    data-testid="task-overlay-edit-button"
                    aria-label={isEditing ? "Cancel editing" : "Edit task"}
                    tooltip={isEditing ? "Cancel editing" : "Edit task"}
                  >
                    <Pencil className="w-4 h-4" />
                  </HeaderIconButton>
                )}
                {/* Archive button */}
                {!readOnly && !isHistoryMode && !isArchived && (
                  <HeaderIconButton
                    variant="ghost"
                    size="icon-sm"
                    onClick={handleArchive}
                    disabled={isArchiving}
                    data-testid="task-overlay-archive-button"
                    aria-label="Archive task"
                    tooltip="Archive task"
                  >
                    {isArchiving ? (
                      <Loader2 className="w-4 h-4 animate-spin" />
                    ) : (
                      <Archive className="w-4 h-4" />
                    )}
                  </HeaderIconButton>
                )}
                {/* Restore button */}
                {!readOnly && !isHistoryMode && isArchived && (
                  <HeaderIconButton
                    variant="ghost"
                    size="icon-sm"
                    onClick={handleRestore}
                    disabled={isRestoring}
                    data-testid="task-overlay-restore-button"
                    aria-label="Restore task"
                    tooltip="Restore task"
                  >
                    {isRestoring ? (
                      <Loader2 className="w-4 h-4 animate-spin" />
                    ) : (
                      <RotateCcw className="w-4 h-4" />
                    )}
                  </HeaderIconButton>
                )}
                {/* Audit Trail button */}
                <HeaderIconButton
                  variant="ghost"
                  size="icon-sm"
                  onClick={() => setShowAuditTrail(true)}
                  data-testid="task-overlay-audit-trail-button"
                  aria-label="Audit Trail"
                  tooltip="Audit trail"
                >
                  <ScrollText className="w-4 h-4" />
                </HeaderIconButton>
                {/* No standalone close — Back button (or ESC) closes the detail. */}
              </div>
            </TooltipProvider>
          </div>

          {/* History selector for viewing historical states (hidden in edit mode) */}
          {!isEditMode && (
            <TaskHistoryDropdown
              taskId={task.id}
              currentStatus={task.internalStatus}
              onStateSelect={handleHistoryStateSelect}
              selectedState={historyState}
            />
          )}

          {/* History Mode Banner */}
          {isHistoryMode && (
            <div
              data-testid="history-mode-banner"
              className="px-4 py-1.5 flex items-center gap-2 shrink-0"
            >
              <History className="w-3 h-3" style={{ color: "var(--text-muted)" }} />
              <span className="text-[0.6875rem]" style={{ color: "var(--text-muted)" }}>
                Viewing: {historyStageLabel}
              </span>
              <span className="text-[0.625rem]" style={{ color: "var(--text-muted)" }}>
                {new Date(historyState.timestamp).toLocaleString()}
              </span>
              <span className="text-[0.625rem]" style={{ color: "var(--text-muted)" }}>
                {historyTranscriptMessage}
              </span>
            </div>
          )}

          {/* Scrollable Content */}
          {isEditMode ? (
            /* Edit Mode - No ScrollArea, form handles its own layout */
            <div className="flex-1 flex flex-col overflow-auto px-6 py-4">
              <div
                data-testid="task-detail-content-frame"
                className={cn("flex flex-1 flex-col", contentFrameClassName)}
              >
                <TaskEditForm
                  task={task}
                  onSave={handleSave}
                  onCancel={() => setIsEditing(false)}
                  isSaving={updateMutation.isPending}
                />
              </div>
            </div>
          ) : (
            /* Read-only View */
            <ScrollArea className="flex-1">
              <div
                data-testid="task-detail-content-frame"
                className={cn("px-6 py-4", contentFrameClassName)}
              >
                <ErrorBoundary>
                  <AgentsTaskDetailPanel
                    task={task}
                    showHeader={false}
                    showContext={true}
                    showHistory={true}
                    useViewRegistry={true}
                    readOnly={readOnly}
                    {...(isHistoryMode && viewStatus ? { viewAsStatus: viewStatus } : {})}
                    {...(isHistoryMode && historyState?.timestamp
                      ? { viewTimestamp: historyState.timestamp }
                      : {})}
                    {...(isHistoryMode && historyState?.conversationId
                      ? { viewConversationId: historyState.conversationId }
                      : {})}
                    {...(isHistoryMode && historyState?.agentRunId
                      ? { viewAgentRunId: historyState.agentRunId }
                      : {})}
                  />
                </ErrorBoundary>
              </div>
            </ScrollArea>
          )}

          {/* Execution Control Bar - current mode only */}
          {footer && !readOnly && !isHistoryMode && (
            <div className="flex-shrink-0">
              {footer}
            </div>
          )}
        </div>
      </div>

      {/* Archive/Restore Confirmation Dialog */}
      <ConfirmationDialog {...confirmationDialogProps} />

      {/* Audit Trail Dialog */}
      <AuditTrailDialog
        taskId={task.id}
        isOpen={showAuditTrail}
        onClose={() => setShowAuditTrail(false)}
      />
    </>
  );
}
