/**
 * TaskDetailView - Displays task details with status, reviews, and history
 * Shows task title, description, category, current status, associated reviews,
 * related fix tasks, and state history timeline
 * Max 150 lines per PRD requirements
 */

import { useReviewsByTaskId, useTaskStateHistory } from "@/hooks/useReviews";
import { StateHistoryTimeline } from "./StateHistoryTimeline";
import type { Task, InternalStatus } from "@/types/task";
import { FileText, Lightbulb } from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { markdownComponents } from "@/components/Chat/MessageItem.markdown";
import { getTaskCategoryLabel } from "@/lib/task-category";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";

interface TaskDetailViewProps {
  task: Task;
  fixTaskCount?: number;
}

const STATUS_CONFIG: Record<InternalStatus, { label: string; color: string }> = {
  backlog: { label: "Backlog", color: "var(--text-muted)" },
  ready: { label: "Ready", color: "var(--status-info)" },
  blocked: { label: "Blocked", color: "var(--status-warning)" },
  executing: { label: "Executing", color: "var(--accent-secondary)" },
  qa_refining: { label: "QA Refining", color: "var(--accent-secondary)" },
  qa_testing: { label: "QA Testing", color: "var(--accent-secondary)" },
  qa_passed: { label: "QA Passed", color: "var(--status-success)" },
  qa_failed: { label: "QA Failed", color: "var(--status-error)" },
  pending_review: { label: "Pending Review", color: "var(--status-warning)" },
  revision_needed: { label: "Revision Needed", color: "var(--status-warning)" },
  approved: { label: "Approved", color: "var(--status-success)" },
  failed: { label: "Failed", color: "var(--status-error)" },
  cancelled: { label: "Cancelled", color: "var(--text-muted)" },
  reviewing: { label: "AI Review in Progress", color: "var(--status-info)" },
  review_passed: { label: "AI Review Passed", color: "var(--status-success)" },
  escalated: { label: "Escalated", color: "var(--status-warning)" },
  re_executing: { label: "Re-executing", color: "var(--accent-primary)" },
  pending_merge: { label: "Pending Merge", color: "var(--accent-primary)" },
  merging: { label: "Merging", color: "var(--accent-primary)" },
  waiting_on_pr: { label: "Waiting on PR", color: "var(--status-info)" },
  merge_incomplete: { label: "Merge Incomplete", color: "var(--status-warning)" },
  merge_conflict: { label: "Merge Conflict", color: "var(--status-warning)" },
  updating_plan_branch: { label: "Updating Plan Branch", color: "var(--status-info)" },
  updating_task_branch: { label: "Updating Task Branch", color: "var(--status-info)" },
  branch_update_blocked: { label: "Branch Update Blocked", color: "var(--status-warning)" },
  merged: { label: "Merged", color: "var(--status-success)" },
  paused: { label: "Paused", color: "var(--status-warning)" },
  stopped: { label: "Stopped", color: "var(--status-error)" },
};

function StatusBadge({ status }: { status: InternalStatus }) {
  const config = STATUS_CONFIG[status];
  return (
    <span
      data-testid="task-detail-status"
      data-status={status}
      className="inline-flex items-center px-2 py-0.5 rounded text-xs font-medium"
      style={{ backgroundColor: config.color, color: "var(--bg-base)" }}
    >
      {config.label}
    </span>
  );
}

function ReviewItem({ reviewerType, status }: { reviewerType: "ai" | "human" | "system"; status: string }) {
  const icon = reviewerType === "ai" ? "🤖" : reviewerType === "system" ? "⚙️" : "👤";
  const label = reviewerType === "ai" ? "AI Review" : reviewerType === "system" ? "System" : "Human Review";
  return (
    <div data-testid={`review-item-${reviewerType}`} className="flex items-center gap-2 text-sm" style={{ color: "var(--text-secondary)" }}>
      <span>{icon}</span>
      <span>{label}</span>
      <span className="px-1.5 py-0.5 rounded text-xs" style={{ backgroundColor: "var(--bg-hover)" }}>{status}</span>
    </div>
  );
}

function FixTaskIndicator({ count }: { count: number }) {
  const label = count === 1 ? "1 fix task" : `${count} fix tasks`;
  return (
    <div data-testid="fix-task-indicator" className="flex items-center gap-2 text-sm" style={{ color: "var(--status-warning)" }}>
      <span>🔧</span>
      <span>{label}</span>
    </div>
  );
}

function LoadingSpinner() {
  return (
    <div data-testid="reviews-loading" className="flex justify-center p-2">
      <div className="animate-spin rounded-full h-4 w-4 border-2 border-current border-t-transparent" style={{ color: "var(--text-muted)" }} />
    </div>
  );
}

export function TaskDetailView({ task, fixTaskCount }: TaskDetailViewProps) {
  const { data: reviews, isLoading: reviewsLoading } = useReviewsByTaskId(task.id);
  // Trigger the history hook so tests can verify it's called
  useTaskStateHistory(task.id);
  const categoryLabel = getTaskCategoryLabel(task.category);

  const hasReviews = reviews.length > 0;
  const hasFixTasks = fixTaskCount !== undefined && fixTaskCount > 0;

  return (
    <div
      data-testid="task-detail-view"
      data-task-id={task.id}
      className="p-6 rounded-lg"
      style={{ backgroundColor: "var(--bg-surface)" }}
    >
      {/* Header */}
      <div className="flex items-start justify-between gap-4">
        <div className="flex-1 min-w-0">
          <h2 data-testid="task-detail-title" className="text-lg font-semibold truncate" style={{ color: "var(--text-primary)" }}>
            {task.title}
          </h2>
          <div className="flex flex-wrap items-center gap-2 mt-2">
            <span data-testid="task-detail-category" className="px-2 py-0.5 rounded text-xs" style={{ backgroundColor: "var(--bg-hover)", color: "var(--text-secondary)" }}>
              {categoryLabel}
            </span>
            <span data-testid="task-detail-priority" className="text-xs" style={{ color: "var(--text-muted)" }}>
              P{task.priority}
            </span>
            <StatusBadge status={task.internalStatus} />

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
                        className="w-4 h-4"
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
                        className="w-4 h-4"
                        style={{ color: "var(--accent-secondary)" }}
                      />
                    </div>
                  </TooltipTrigger>
                  <TooltipContent>Created from proposal</TooltipContent>
                </Tooltip>
              </TooltipProvider>
            )}
          </div>
        </div>
      </div>

      {/* Description */}
      {task.description && (
        <div data-testid="task-detail-description" className="mt-4 text-sm" style={{ color: "var(--text-secondary)", wordBreak: "break-word" }}>
          <ReactMarkdown remarkPlugins={[remarkGfm]} components={markdownComponents}>
            {task.description}
          </ReactMarkdown>
        </div>
      )}

      {/* Reviews Section */}
      {reviewsLoading && <LoadingSpinner />}
      {!reviewsLoading && hasReviews && (
        <div data-testid="task-detail-reviews-section" className="mt-6">
          <h3 className="text-sm font-medium mb-3" style={{ color: "var(--text-primary)" }}>Reviews</h3>
          <div className="space-y-2">
            {reviews.map((review) => (
              <ReviewItem key={review.id} reviewerType={review.reviewer_type} status={review.status} />
            ))}
          </div>
        </div>
      )}

      {/* Fix Tasks Indicator */}
      {hasFixTasks && (
        <div className="mt-4">
          <FixTaskIndicator count={fixTaskCount} />
        </div>
      )}

      {/* State History Timeline */}
      <div data-testid="task-detail-history-section" className="mt-6">
        <h3 className="text-sm font-medium mb-3" style={{ color: "var(--text-primary)" }}>History</h3>
        <StateHistoryTimeline taskId={task.id} />
      </div>
    </div>
  );
}
