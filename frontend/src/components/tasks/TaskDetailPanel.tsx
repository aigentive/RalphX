/**
 * TaskDetailPanel - State-aware task detail content using View Registry Pattern
 *
 * This component selects and renders the appropriate detail view component
 * based on the task's internal status. Each status maps to a specialized
 * view that shows relevant information and actions for that state.
 *
 * View Registry Pattern:
 * - Each InternalStatus maps to a specific detail view component
 * - Specialized views: ExecutionTaskDetail, ReviewingTaskDetail, etc.
 * - BasicTaskDetail serves as the fallback for simple states
 *
 * Design spec: specs/design/refined-studio-patterns.md
 * - Refined Studio aesthetic with layered depth
 * - Gradient backgrounds and premium shadows
 */

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { RecoveryPromptDialog } from "@/components/recovery/RecoveryPromptDialog";
import { useReviewsByTaskId } from "@/hooks/useReviews";
import { StateHistoryTimeline } from "./StateHistoryTimeline";
import { TaskContextPanel } from "./TaskContextPanel";
import { StepList } from "./StepList";
import { useTaskSteps } from "@/hooks/useTaskSteps";
import type { Task, InternalStatus } from "@/types/task";
import type { ComponentType } from "react";
import { Bot, User, Settings, Loader2, FileText } from "lucide-react";
import { useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { markdownComponents } from "@/components/Chat/MessageItem.markdown";
import { getTaskCategoryLabel } from "@/lib/task-category";
import { withAlpha } from "@/lib/theme-colors";
import { TaskDetailContextProvider } from "./detail-views/shared/TaskDetailContextProvider";
import type { TaskDetailViewMode } from "./detail-views/shared/TaskDetailContext";

// Import state-specific detail view components
import {
  BasicTaskDetail,
  RevisionTaskDetail,
  ExecutionTaskDetail,
  ReviewingTaskDetail,
  HumanReviewTaskDetail,
  EscalatedTaskDetail,
  WaitingTaskDetail,
  CompletedTaskDetail,
  MergingTaskDetail,
  MergeConflictTaskDetail,
  MergeIncompleteTaskDetail,
  MergedTaskDetail,
} from "./detail-views";

interface TaskDetailPanelProps {
  task: Task;
  showHeader?: boolean;
  showContext?: boolean;
  showHistory?: boolean;
  /** Use state-specific view from registry instead of default panel */
  useViewRegistry?: boolean;
  /** Override status for view registry lookup (used in history mode) */
  viewAsStatus?: InternalStatus;
  /** Timestamp for historical view context */
  viewTimestamp?: string;
  /** Conversation ID for historical state context */
  viewConversationId?: string | undefined;
  /** Agent run ID for historical state context */
  viewAgentRunId?: string | undefined;
}

/**
 * Props interface for state-specific detail view components
 */
interface TaskDetailProps {
  task: Task;
  /** True when viewing a historical state - disables action buttons */
  isHistorical?: boolean;
  /** Status override for historical view rendering */
  viewStatus?: InternalStatus | undefined;
}

/**
 * View Registry Pattern - Maps InternalStatus to specialized detail view components
 *
 * Each status maps to a view component that shows relevant information and actions:
 * - BasicTaskDetail: Simple states (backlog, ready, blocked, qa_*, failed, cancelled)
 * - ExecutionTaskDetail: Active work states (executing, re_executing)
 * - RevisionTaskDetail: Needs revision state
 * - WaitingTaskDetail: Pending review state
 * - ReviewingTaskDetail: AI review in progress
 * - HumanReviewTaskDetail: AI approved, awaiting human
 * - CompletedTaskDetail: Successfully completed tasks
 */
const TASK_DETAIL_VIEWS: Record<
  InternalStatus,
  ComponentType<TaskDetailProps>
> = {
  // Idle states - use basic view
  backlog: BasicTaskDetail,
  ready: BasicTaskDetail,
  blocked: BasicTaskDetail,
  // Execution states - use execution view
  executing: ExecutionTaskDetail,
  re_executing: ExecutionTaskDetail,
  // QA states - use basic view (no specialized QA view yet)
  qa_refining: BasicTaskDetail,
  qa_testing: BasicTaskDetail,
  qa_passed: BasicTaskDetail,
  qa_failed: BasicTaskDetail,
  // Review states - specialized views
  pending_review: WaitingTaskDetail,
  reviewing: ReviewingTaskDetail,
  review_passed: HumanReviewTaskDetail,
  escalated: EscalatedTaskDetail,
  revision_needed: RevisionTaskDetail,
  // Approval leads to merge
  approved: CompletedTaskDetail,
  // Merge states - specialized views
  pending_merge: MergingTaskDetail,
  merging: MergingTaskDetail,
  waiting_on_pr: MergingTaskDetail,
  merge_incomplete: MergeIncompleteTaskDetail,
  merge_conflict: MergeConflictTaskDetail,
  // Terminal states
  merged: MergedTaskDetail,
  failed: BasicTaskDetail,
  cancelled: BasicTaskDetail,
  // Suspended states
  paused: BasicTaskDetail,
  stopped: BasicTaskDetail,
};

// Priority colors matching design spec
const PRIORITY_COLORS: Record<number, { bg: string; text: string }> = {
  1: { bg: "var(--status-error)", text: "white" },
  2: { bg: "var(--accent-primary)", text: "white" },
  3: { bg: "var(--status-warning)", text: "var(--bg-base)" },
  4: { bg: "var(--bg-hover)", text: "var(--text-secondary)" },
};

// Status badge configuration matching design spec
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

const DEFAULT_PRIORITY_COLOR = { bg: "var(--bg-hover)", text: "var(--text-secondary)" };

function PriorityBadge({ priority }: { priority: number }) {
  const colors = PRIORITY_COLORS[priority] ?? DEFAULT_PRIORITY_COLOR;
  return (
    <span
      data-testid="task-detail-priority"
      className="inline-flex items-center px-1.5 py-0.5 rounded text-[0.625rem] font-mono font-medium"
      style={{ backgroundColor: colors.bg, color: colors.text }}
    >
      P{priority}
    </span>
  );
}

function StatusBadge({ status }: { status: InternalStatus }) {
  const config = STATUS_CONFIG[status];
  return (
    <Badge
      data-testid="task-detail-status"
      data-status={status}
      className="rounded px-1.5 py-0.5 text-[0.625rem] font-medium border-0"
      style={{ backgroundColor: config.bg, color: config.text }}
    >
      {config.label}
    </Badge>
  );
}

function ReviewCard({
  reviewerType,
  status,
}: {
  reviewerType: "ai" | "human" | "system";
  status: string;
}) {
  const Icon = reviewerType === "ai" ? Bot : reviewerType === "system" ? Settings : User;
  const label = reviewerType === "ai" ? "AI Review" : reviewerType === "system" ? "System" : "Human Review";

  const defaultStatusColor = { bg: "var(--overlay-weak)", text: withAlpha("var(--text-primary)", 50) };
  const statusColors: Record<string, { bg: string; text: string }> = {
    pending: defaultStatusColor,
    approved: {
      bg: "var(--status-success-muted)",
      text: "var(--status-success)",
    },
    changes_requested: {
      bg: "var(--status-warning-muted)",
      text: "var(--status-warning)",
    },
    rejected: { bg: "var(--status-error-muted)", text: "var(--status-error)" },
  };

  const statusColor = statusColors[status] ?? defaultStatusColor;

  return (
    <div
      data-testid={`review-item-${reviewerType}`}
      className="flex items-center justify-between p-2.5 rounded-lg"
      style={{
        background: `linear-gradient(180deg, ${withAlpha("var(--bg-elevated)", 90)} 0%, ${withAlpha("var(--bg-surface)", 95)} 100%)`,
        border: "1px solid var(--overlay-weak)",
      }}
    >
      <div className="flex items-center gap-2">
        <Icon className="w-3.5 h-3.5 text-text-primary/50" />
        <span className="text-[0.8125rem] font-medium text-text-primary/80">
          {label}
        </span>
      </div>
      <Badge
        className="rounded px-1.5 py-0.5 text-[0.625rem] font-medium border-0 capitalize"
        style={{ backgroundColor: statusColor.bg, color: statusColor.text }}
      >
        {status.replace("_", " ")}
      </Badge>
    </div>
  );
}

function SectionTitle({ children }: { children: React.ReactNode }) {
  return (
    <h3 className="text-[0.8125rem] font-medium mb-2.5 text-text-primary/80">
      {children}
    </h3>
  );
}

export function TaskDetailPanel({
  task,
  showHeader = true,
  showContext: showContextProp = false,
  showHistory: showHistoryProp = true,
  useViewRegistry = false,
  viewAsStatus,
  viewTimestamp,
  viewConversationId,
  viewAgentRunId,
}: TaskDetailPanelProps) {
  const [showContext, setShowContext] = useState(showContextProp);

  // Fetch reviews - must be called unconditionally (hooks rules)
  const { data: reviews, isLoading: reviewsLoading } = useReviewsByTaskId(task.id);

  // Fetch steps - must be called unconditionally (hooks rules)
  const { data: steps, isLoading: stepsLoading } = useTaskSteps(task.id);
  const categoryLabel = getTaskCategoryLabel(task.category);

  // If using View Registry Pattern, render the appropriate state-specific component
  // This check must come AFTER all hooks to satisfy React hooks rules
  if (useViewRegistry) {
    // Use viewAsStatus for history mode, otherwise use current status
    const statusForView = viewAsStatus ?? task.internalStatus;
    const ViewComponent =
      TASK_DETAIL_VIEWS[statusForView] ?? BasicTaskDetail;
    // Pass isHistorical when viewing a historical state (viewAsStatus is set)
    const isHistorical = viewAsStatus !== undefined;
    const viewMode: TaskDetailViewMode = isHistorical
      ? {
          kind: "historical",
          status: statusForView,
          timestamp: viewTimestamp ?? task.updatedAt,
          conversationId: viewConversationId,
          agentRunId: viewAgentRunId,
        }
      : { kind: "current" };
    if (statusForView === "reviewing") {
      return (
        <TaskDetailContextProvider task={task} viewMode={viewMode}>
          <ReviewingTaskDetail
            key={`reviewing-${isHistorical ? "hist" : "curr"}`}
            task={task}
            isHistorical={isHistorical}
            viewTimestamp={viewMode.kind === "historical" ? viewMode.timestamp : undefined}
          />
        </TaskDetailContextProvider>
      );
    }
    return (
      <TaskDetailContextProvider task={task} viewMode={viewMode}>
        <ViewComponent
          key={`${statusForView}-${isHistorical ? "hist" : "curr"}`}
          task={task}
          isHistorical={isHistorical}
          viewStatus={statusForView}
        />
      </TaskDetailContextProvider>
    );
  }

  const hasReviews = reviews.length > 0;
  const hasContext = !!(task.sourceProposalId || task.planArtifactId);
  const hasSteps = (steps?.length ?? 0) > 0;

  return (
    <div
      data-testid="task-detail-panel"
      data-task-id={task.id}
      className="space-y-6"
    >
      <RecoveryPromptDialog surface="task_detail" taskId={task.id} />
      {/* Header with priority, title, category, status - optional */}
      {showHeader && (
        <div className="space-y-2">
          <div className="min-w-0">
            <h2
              data-testid="task-detail-title"
              className="text-base font-semibold text-text-primary/90"
              style={{
                letterSpacing: "-0.02em",
                lineHeight: "1.3",
              }}
            >
              {task.title}
            </h2>
            <div className="flex flex-wrap items-center gap-1.5 mt-1.5">
              <PriorityBadge priority={task.priority} />
              <span
                data-testid="task-detail-category"
                className="px-1.5 py-0.5 rounded text-[10px] font-medium"
                style={{
                  backgroundColor: "var(--overlay-weak)",
                  border: "1px solid var(--border-subtle)",
                  color: withAlpha("var(--text-primary)", 60),
                }}
              >
                {categoryLabel}
              </span>
              <StatusBadge status={task.internalStatus} />
            </div>
          </div>
        </div>
      )}

      {/* View Context Button */}
      {hasContext && (
        <div>
          <Button
            variant="outline"
            size="sm"
            onClick={() => setShowContext(!showContext)}
            data-testid="view-context-button"
            className="w-full justify-center"
          >
            <FileText className="h-4 w-4 mr-2" />
            {showContext ? "Hide Context" : "View Context"}
          </Button>
        </div>
      )}

      {/* Task Context Panel */}
      {showContext && hasContext && (
        <div data-testid="task-context-section">
          <TaskContextPanel taskId={task.id} />
        </div>
      )}

      {/* Description Section */}
      {task.description ? (
        <div
          data-testid="task-detail-description"
          className="text-[0.8125rem] text-text-primary/60"
          style={{
            lineHeight: "1.6",
            wordBreak: "break-word",
          }}
        >
          <ReactMarkdown remarkPlugins={[remarkGfm]} components={markdownComponents}>
            {task.description}
          </ReactMarkdown>
        </div>
      ) : (
        <p className="text-[0.8125rem] italic text-text-primary/35">
          No description provided
        </p>
      )}

      {/* Steps Section */}
      {stepsLoading && (
        <div className="flex justify-center py-4">
          <Loader2
            className="w-6 h-6 animate-spin"
            style={{ color: "var(--text-muted)" }}
          />
        </div>
      )}
      {!stepsLoading && hasSteps && (
        <div data-testid="task-detail-steps-section">
          <SectionTitle>Steps</SectionTitle>
          <StepList taskId={task.id} editable={false} />
        </div>
      )}

      {/* Reviews Section */}
      {reviewsLoading && (
        <div
          data-testid="reviews-loading"
          className="flex justify-center py-4"
        >
          <Loader2
            className="w-6 h-6 animate-spin"
            style={{ color: "var(--text-muted)" }}
          />
        </div>
      )}
      {!reviewsLoading && hasReviews && (
        <div data-testid="task-detail-reviews-section">
          <SectionTitle>Reviews</SectionTitle>
          <div className="space-y-2">
            {reviews.map((review) => (
              <ReviewCard
                key={review.id}
                reviewerType={review.reviewer_type}
                status={review.status}
              />
            ))}
          </div>
        </div>
      )}

      {/* History Section */}
      {showHistoryProp && (
        <div data-testid="task-detail-history-section">
          <SectionTitle>History</SectionTitle>
          <StateHistoryTimeline taskId={task.id} />
        </div>
      )}
    </div>
  );
}

// Export the registry for external use (e.g., direct view lookup)
export { TASK_DETAIL_VIEWS };
export type { TaskDetailProps };
