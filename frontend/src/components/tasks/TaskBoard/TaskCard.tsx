/**
 * TaskCard - Draggable task card for the kanban board
 *
 * Design: v29a Kanban
 * - Flat card surfaces, no gradients or glows
 * - No left status stripe
 * - Full-card status tints for warning and completed states
 *
 * Styling utilities extracted to TaskCard.utils.ts
 */

import { useDraggable } from "@dnd-kit/core";
import { GripVertical, FileText, Lightbulb, Clock, Ban, GitBranch, GitPullRequest } from "lucide-react";
import { useMemo, useCallback } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { markdownComponents } from "@/components/Chat/MessageItem.markdown";
import type { Task } from "@/types/task";
import { StatusBadge, type ReviewStatus } from "@/components/ui/StatusBadge";
import { TaskQABadge } from "@/components/qa/TaskQABadge";
import { Badge } from "@/components/ui/badge";
import type { QAPrepStatus } from "@/types/qa-config";
import type { QAOverallStatus } from "@/types/qa";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { TaskCardContextMenu } from "@/components/tasks/TaskCardContextMenu";
import type { GroupInfo } from "@/lib/task-actions";
import { useTaskMutation } from "@/hooks/useTaskMutation";
import { useIdeationStore } from "@/stores/ideationStore";
import { useCreateIdeationSession } from "@/hooks/useIdeation";
import { navigateToIdeationSession, openTaskInAgents } from "@/lib/navigation";
import { toast } from "sonner";
import { useTaskExecutionState, formatDuration } from "@/hooks/useTaskExecutionState";
import { usePlanBranchForTask } from "@/hooks/usePlanBranchForTask";
import { StepProgressBar } from "@/components/tasks/StepProgressBar";
import { TaskStatusBadge } from "./TaskStatusBadge";
import { getCardStyles, isDraggableStatus } from "./TaskCard.utils";
import { getTaskCategoryLabel } from "@/lib/task-category";

export type TaskCardDisplayMode = "default" | "mini";

interface TaskCardProps {
  task: Task;
  isDragging?: boolean;
  isSelected?: boolean;
  isHidden?: boolean;
  displayMode?: TaskCardDisplayMode;
  reviewStatus?: ReviewStatus;
  /** Whether this task needs QA */
  needsQA?: boolean;
  /** QA prep status */
  prepStatus?: QAPrepStatus;
  /** Overall QA test status */
  testStatus?: QAOverallStatus;
  hasCheckpoint?: boolean;
  /** Number of revision attempts (for re_executing state badge) */
  revisionCount?: number;
  /** Group context for showing group actions in task context menu */
  groupInfo?: GroupInfo;
  /** Optional host-owned task selection handler for embedded task surfaces. */
  onSelect?: (taskId: string) => void;
  readOnly?: boolean;
}

function CheckpointIndicator() {
  return (
    <Badge
      data-testid="checkpoint-indicator"
      className="text-[0.5625rem] px-1.5 py-px"
      style={{
        backgroundColor: "var(--status-warning-muted)",
        color: "var(--status-warning)",
        border: "none",
      }}
    >
      Checkpoint
    </Badge>
  );
}

export function TaskCard({
  task,
  isDragging,
  isSelected,
  isHidden,
  displayMode = "default",
  reviewStatus,
  needsQA,
  prepStatus,
  testStatus,
  hasCheckpoint,
  revisionCount,
  groupInfo,
  onSelect,
  readOnly = false,
}: TaskCardProps) {
  const { attributes, listeners, setNodeRef, transform } = useDraggable({
    id: task.id,
    disabled: readOnly,
  });

  // Ideation Store and mutation
  const addSession = useIdeationStore((state) => state.addSession);
  const setActiveSession = useIdeationStore((state) => state.setActiveSession);
  const createSession = useCreateIdeationSession();

  // Execution state
  const executionState = useTaskExecutionState(task.id);

  // PR mode — fetch plan branch only for plan_merge tasks
  const isPlanMerge = task.category === "plan_merge";
  const { data: planBranch } = usePlanBranchForTask(task.id, { enabled: isPlanMerge });
  const hasPrContext = isPlanMerge && planBranch?.prNumber != null;
  const prState =
    planBranch?.status === "merged" || task.internalStatus === "merged" || planBranch?.prStatus === "Merged"
      ? "merged"
      : planBranch?.prStatus === "Closed"
      ? "closed"
      : "review";
  const prIndicatorLabel =
    prState === "merged" ? "Merged PR" : prState === "closed" ? "Closed PR" : "Review PR";
  const prIndicatorColor =
    prState === "merged"
      ? "var(--status-success)"
      : prState === "closed"
      ? "var(--status-warning)"
      : "var(--status-info)";
  const prIndicatorTooltip =
    prState === "merged"
      ? `PR #${planBranch?.prNumber} has been merged`
      : prState === "closed"
      ? `PR #${planBranch?.prNumber} was closed`
      : `PR #${planBranch?.prNumber} - waiting for GitHub review or merge`;
  const categoryLabel = getTaskCategoryLabel(task.category);
  const isMiniDisplay = displayMode === "mini";

  // Mutations
  const {
    archiveMutation,
    restoreMutation,
    cleanupTaskMutation,
    moveMutation,
    pauseMutation,
    resumeMutation,
    blockMutation,
    unblockMutation,
  } = useTaskMutation(task.projectId);

  // Check if task is archived
  const isArchived = task.archivedAt !== null;

  // Determine if task is draggable based on internal status
  const isDraggable = useMemo(() => isDraggableStatus(task.internalStatus), [task.internalStatus]);

  // Drag styling:
  // - isHidden: briefly hide when moving to a different column (prevents duplicate)
  // - Otherwise: always fully visible (DragOverlay handles the floating preview)
  const dragStyle: React.CSSProperties = {
    ...(transform && { transform: `translate3d(${transform.x}px, ${transform.y}px, 0)` }),
    opacity: isHidden ? 0 : 1,
    transition: 'opacity 150ms ease-out',
  };

  // Build QA badge props conditionally to satisfy exactOptionalPropertyTypes
  const qaBadgeProps = {
    needsQA: needsQA ?? false,
    ...(prepStatus !== undefined && { prepStatus }),
    ...(testStatus !== undefined && { testStatus }),
  };

  // Computed styles using extracted utilities - full-card status surfaces
  const cardStyles = useMemo(
    () => getCardStyles(task.internalStatus, isArchived, !!isDragging, isDraggable, !!isSelected),
    [task.internalStatus, isArchived, isDragging, isDraggable, isSelected]
  );

  // Embedded Agents boards own task selection; legacy hosts resolve through Agents.
  const handleViewDetails = useCallback(() => {
    if (onSelect) {
      onSelect(task.id);
      return;
    }
    void openTaskInAgents(task.id, "kanban", { projectId: task.projectId });
  }, [onSelect, task.id, task.projectId]);

  const handleEdit = handleViewDetails;

  const handleArchive = () => {
    archiveMutation.mutate(task.id);
  };

  const handleRestore = () => {
    restoreMutation.mutate(task.id);
  };

  const handleRemove = () => {
    cleanupTaskMutation.mutate(task.id);
  };

  const handleStatusChange = (newStatus: string) => {
    moveMutation.mutate({ taskId: task.id, toStatus: newStatus });
  };

  const handleBlockWithReason = useCallback(
    (reason?: string) => {
      blockMutation.mutate(reason ? { taskId: task.id, reason } : { taskId: task.id });
    },
    [task.id, blockMutation]
  );

  const handleUnblock = useCallback(() => {
    unblockMutation.mutate(task.id);
  }, [task.id, unblockMutation]);

  const handlePause = useCallback(() => {
    pauseMutation.mutate(task.id);
  }, [task.id, pauseMutation]);

  const handleResume = useCallback(() => {
    resumeMutation.mutate(task.id);
  }, [task.id, resumeMutation]);

  const handleStartExecution = useCallback(() => {
    moveMutation.mutate({ taskId: task.id, toStatus: "executing" });
  }, [task.id, moveMutation]);

  const handleStartIdeation = async () => {
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
      navigateToIdeationSession(session.id);
    } catch (error) {
      console.error("Failed to start ideation:", error);
      toast.error("Failed to start ideation session");
    }
  };

  return (
    <>
      <TaskCardContextMenu
        readOnly={readOnly}
        task={task}
        onViewDetails={handleViewDetails}
        onEdit={handleEdit}
        onArchive={handleArchive}
        onRestore={handleRestore}
        onRemove={handleRemove}
        onStatusChange={handleStatusChange}
        onBlockWithReason={handleBlockWithReason}
        onUnblock={handleUnblock}
        onPause={handlePause}
        onResume={handleResume}
        onStartExecution={handleStartExecution}
        onStartIdeation={handleStartIdeation}
        {...(groupInfo !== undefined && { groupInfo })}
      >
        <div
          ref={setNodeRef}
          {...(isDraggable && !readOnly ? { ...attributes, ...listeners } : {})}
          data-testid={`task-card-${task.id}`}
          onClick={() => {
            handleViewDetails();
          }}
          className={`group relative px-3 ${isMiniDisplay ? "py-2" : "py-2.5"} focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--focus-ring)] ${isArchived ? "opacity-50" : ""} ${!isDraggable ? "cursor-default" : ""}`}
          style={{ ...cardStyles, ...dragStyle }}
          tabIndex={0}
        >
      {/* Drag handle - appears on hover over the status badge (hidden if not draggable) */}
      {isDraggable && !isArchived && !readOnly && (
        <div
          data-testid="drag-handle"
          className="absolute right-8 top-2.5 opacity-0 group-hover:opacity-100 transition-opacity cursor-grab"
        >
          <GripVertical className="w-3.5 h-3.5" style={{ color: "var(--text-muted)" }} />
        </div>
      )}

      {/* Card content */}
      <div className="flex flex-col gap-2">
        <div className="flex items-start gap-2">
        {/* Title - v29a compact card heading */}
        <div
          data-testid="task-title"
          className="min-w-0 flex-1 truncate"
          style={{
            fontSize: "0.8125rem",
            fontWeight: 500,
            color: "var(--text-primary)",
            lineHeight: 1.35,
            letterSpacing: 0,
          }}
        >
          {task.title}
        </div>

        {/* Status badge - shown for ALL task states */}
        <div className="shrink-0" data-testid="status-badge-container">
          <TaskStatusBadge
            status={task.internalStatus}
            isArchived={isArchived}
            {...(revisionCount !== undefined && { revisionCount })}
          />
        </div>
        </div>

        {/* Description - 2 line clamp with markdown */}
        {!isMiniDisplay && task.description && (
          <div
            className="line-clamp-2 [&_*]:!mb-0 [&_*]:!mt-0"
            style={{
              fontSize: "0.7188rem",
              color: "var(--text-muted)",
              lineHeight: 1.45,
              letterSpacing: 0,
            }}
          >
            <ReactMarkdown remarkPlugins={[remarkGfm]} components={markdownComponents}>
              {task.description}
            </ReactMarkdown>
          </div>
        )}

        {/* Blocked reason indicator - yellow pill style */}
        {!isMiniDisplay && task.internalStatus === "blocked" && task.blockedReason && (
          <TooltipProvider>
            <Tooltip>
              <TooltipTrigger asChild>
                <div
                  data-testid="blocked-reason-indicator"
                  className="flex items-center gap-1.5 mt-1.5 px-2 py-0.5 rounded-full text-xs max-w-full"
                  style={{
                    backgroundColor: "var(--status-warning-muted)",
                    color: "var(--status-warning)",
                  }}
                >
                  <Ban className="w-3 h-3 flex-shrink-0" />
                  <span className="truncate">{task.blockedReason}</span>
                </div>
              </TooltipTrigger>
              <TooltipContent side="bottom" className="max-w-xs">
                <p className="text-sm">{task.blockedReason}</p>
              </TooltipContent>
            </Tooltip>
          </TooltipProvider>
        )}

        {/* Badge row - simple, muted */}
        {!isMiniDisplay && (
        <div className="flex flex-wrap items-center gap-1.5">
          <span
            className="inline-flex h-5 items-center rounded-full px-2"
            style={{
              background: "var(--bg-hover)",
              border: "1px solid var(--border-default)",
              fontFamily: "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace",
              fontSize: "0.6562rem",
              fontWeight: 500,
              color: "var(--text-secondary)",
              textTransform: "capitalize",
            }}
          >
            {categoryLabel}
          </span>
          {reviewStatus && <StatusBadge type="review" status={reviewStatus} />}
          <TaskQABadge {...qaBadgeProps} />
          {hasCheckpoint && <CheckpointIndicator />}

          {/* Artifact indicators */}
          {task.planArtifactId && (
            <TooltipProvider>
              <Tooltip>
                <TooltipTrigger asChild>
                  <div
                    data-testid="plan-artifact-indicator"
                    className="inline-flex items-center justify-center"
                  >
                    <FileText
                      className="w-3.5 h-3.5"
                      style={{ color: "var(--accent-primary)" }}
                    />
                  </div>
                </TooltipTrigger>
                <TooltipContent>Has implementation plan</TooltipContent>
              </Tooltip>
            </TooltipProvider>
          )}

          {task.sourceProposalId && (
            <TooltipProvider>
              <Tooltip>
                <TooltipTrigger asChild>
                  <div
                    data-testid="source-proposal-indicator"
                    className="inline-flex items-center justify-center"
                  >
                    <Lightbulb
                      className="w-3.5 h-3.5"
                      style={{ color: "var(--accent-secondary)" }}
                    />
                  </div>
                </TooltipTrigger>
                <TooltipContent>Created from proposal</TooltipContent>
              </Tooltip>
            </TooltipProvider>
          )}

          {/* Branch indicator - shown for tasks with active branch */}
          {task.taskBranch && (
            <TooltipProvider>
              <Tooltip>
                <TooltipTrigger asChild>
                  <div
                    data-testid="branch-indicator"
                    className="inline-flex items-center gap-1"
                    style={{ color: "var(--text-secondary)" }}
                  >
                    <GitBranch className="w-3 h-3 flex-shrink-0" />
                    <span
                      className="text-[0.625rem] truncate max-w-[80px]"
                      style={{
                        fontFamily: "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace",
                      }}
                    >
                      {task.taskBranch.split("/").pop()}
                    </span>
                  </div>
                </TooltipTrigger>
                <TooltipContent side="bottom" className="max-w-xs">
                  <p className="text-xs font-mono">{task.taskBranch}</p>
                </TooltipContent>
              </Tooltip>
            </TooltipProvider>
          )}

          {/* PR indicator — shown when a plan merge task is linked to a PR */}
          {hasPrContext && (
            <TooltipProvider>
              <Tooltip>
                <TooltipTrigger asChild>
                  <div
                    data-testid="pr-mode-indicator"
                    className="inline-flex items-center gap-1"
                    style={{ color: prIndicatorColor }}
                  >
                    <GitPullRequest className="w-3 h-3 flex-shrink-0" />
                    <span className="text-[0.625rem]">{prIndicatorLabel}</span>
                  </div>
                </TooltipTrigger>
                <TooltipContent side="bottom">
                  {prIndicatorTooltip}
                </TooltipContent>
              </Tooltip>
            </TooltipProvider>
          )}
        </div>
        )}

        {/* Step progress indicator - shown for all post-execution statuses */}
        {(task.internalStatus === "executing" ||
          task.internalStatus === "re_executing" ||
          task.internalStatus.startsWith("qa_") ||
          task.internalStatus === "pending_review" ||
          task.internalStatus === "reviewing" ||
          task.internalStatus === "review_passed" ||
          task.internalStatus === "escalated" ||
          task.internalStatus === "revision_needed" ||
          task.internalStatus === "approved" ||
          task.internalStatus === "merged") && (
          <div className="flex items-center gap-2" data-testid="step-progress-footer">
            <StepProgressBar taskId={task.id} compact={true} internalStatus={task.internalStatus} />

            {/* Duration badge - shown when executing */}
            {!isMiniDisplay && (task.internalStatus === "executing" || task.internalStatus === "re_executing") && executionState.duration !== null && (
              <div
                className="flex items-center gap-1 text-[0.6875rem]"
                style={{ color: "var(--text-secondary)" }}
              >
                <Clock className="w-3 h-3" />
                <span>{formatDuration(executionState.duration)}</span>
              </div>
            )}
          </div>
        )}
      </div>
    </div>
      </TaskCardContextMenu>

    </>
  );
}
