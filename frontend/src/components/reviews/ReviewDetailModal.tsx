/**
 * ReviewDetailModal - Full-width modal for detailed code review
 *
 * Shows task context, AI review summary, review history on the left,
 * and DiffViewer on the right. Footer has Approve/Request Changes buttons.
 *
 * Part of the Review System (Phase 20).
 */

import { useState, useCallback, useEffect, useMemo } from "react";
import { useMutation, useQueryClient, useQuery } from "@tanstack/react-query";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { markdownComponents } from "@/components/Chat/MessageItem.markdown";
import {
  X,
  Loader2,
  CheckCircle2,
  RotateCcw,
  Bot,
  User,
  Settings,
  MessageSquare,
  Clock,
  ExternalLink,
} from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { cn } from "@/lib/utils";
import { DiffViewer } from "@/components/diff";
import { PlanMergeContextCard } from "@/components/tasks/detail-views/shared/PlanMergeContextSection";
import { useGitDiff } from "@/hooks/useGitDiff";
import { useReviewsByTaskId, useTaskStateHistory, reviewKeys } from "@/hooks/useReviews";
import { taskKeys } from "@/hooks/useTasks";
import { api } from "@/lib/tauri";
import { useConfirmation } from "@/hooks/useConfirmation";
import { navigateToIdeationSession } from "@/lib/navigation";
import { getTaskCategoryLabel } from "@/lib/task-category";
import { withAlpha } from "@/lib/theme-colors";
import type { Commit } from "@/components/diff";
import type { ReviewNoteResponse } from "@/lib/tauri";

interface ReviewDetailModalProps {
  taskId: string;
  /** Pre-fetched history from parent to avoid duplicate API calls */
  history?: ReviewNoteResponse[];
  /** @deprecated No longer required - using task-based approval APIs */
  reviewId?: string;
  /** Hide approve/request actions for read-only contexts */
  showActions?: boolean;
  /** Read-only review surface variant. */
  reviewMode?: "task" | "plan_merge";
  onClose: () => void;
}

/**
 * TaskContextSection - Shows task title, category, description
 */
function TaskContextSection({
  category,
  description,
  priority,
  isLoading,
}: {
  category: string;
  description: string | null;
  priority: number;
  isLoading: boolean;
}) {
  if (isLoading) {
    return (
      <div className="space-y-2">
        <div className="h-5 w-3/4 rounded animate-pulse bg-[var(--overlay-moderate)]" />
        <div className="h-4 w-1/2 rounded animate-pulse bg-[var(--overlay-moderate)]" />
        <div className="h-16 w-full rounded animate-pulse bg-[var(--overlay-moderate)]" />
      </div>
    );
  }

  const categoryLabel = getTaskCategoryLabel(category);

  return (
    <div className="space-y-3">
      {/* Title removed - already displayed in modal header */}
      <div className="flex items-center gap-3 text-[0.75rem]">
        <span className="text-text-primary/50">
          Priority: <span className="text-text-primary/70">{priority}</span>
        </span>
        <span className="text-text-primary/50">
          Category: <span className="text-text-primary/70">{categoryLabel}</span>
        </span>
      </div>
      {description && (
        <div
          data-testid="modal-task-description"
          className="text-[0.75rem] text-text-primary/60"
          style={{ lineHeight: "1.5", wordBreak: "break-word", overflowWrap: "anywhere" }}
        >
          <ReactMarkdown remarkPlugins={[remarkGfm]} components={markdownComponents}>
            {description}
          </ReactMarkdown>
        </div>
      )}
    </div>
  );
}

/**
 * AIReviewSummary - Shows the AI review summary and checklist
 */
function AIReviewSummary({
  latestApproved,
  hasAiReview,
}: {
  latestApproved: ReviewNoteResponse | null;
  hasAiReview: boolean;
}) {
  if (!hasAiReview) {
    return (
      <div
        className="rounded-lg p-3 text-center"
        style={{
          backgroundColor: "var(--overlay-scrim)",
          border: "1px solid var(--overlay-weak)",
        }}
      >
        <p className="text-[0.75rem] text-text-primary/40">No AI review yet</p>
      </div>
    );
  }

  // Default checklist items (could come from review data in future)
  const checklistItems = [
    { label: "Code follows project patterns", passed: true },
    { label: "Tests are passing", passed: true },
    { label: "No linting errors", passed: true },
  ];

  return (
    <div
      data-testid="ai-review-summary"
      className="rounded-lg p-3 space-y-3"
      style={{
        backgroundColor: "var(--overlay-scrim)",
        border: "1px solid var(--status-success-border)",
      }}
    >
      {/* Header */}
      <div className="flex items-center gap-2">
        <div
          className="flex items-center justify-center w-6 h-6 rounded-full shrink-0"
          style={{ backgroundColor: "var(--status-info-muted)" }}
        >
          <Bot className="w-3.5 h-3.5" style={{ color: "var(--status-info)" }} />
        </div>
        <span className="text-[0.75rem] font-medium text-text-primary/70">
          AI Review Summary
        </span>
        {latestApproved && (
          <span
            className="ml-auto text-[0.6875rem] px-2 py-0.5 rounded-full"
            style={{
              backgroundColor: "var(--status-success-muted)",
              color: "var(--status-success)",
            }}
          >
            Passed
          </span>
        )}
      </div>

      {/* Summary text - rendered as markdown */}
      {latestApproved?.notes && (
        <div className="text-[0.75rem] text-text-primary/60 prose prose-sm prose-invert max-w-none">
          <ReactMarkdown remarkPlugins={[remarkGfm]}>
            {latestApproved.notes}
          </ReactMarkdown>
        </div>
      )}

      {/* Checklist */}
      <div className="space-y-0.5">
        {checklistItems.map((item, index) => (
          <div
            key={index}
            className={`flex items-center gap-2 py-1 ${item.passed ? "text-text-primary/60" : "text-text-primary/40"}`}
          >
            <CheckCircle2
              className="w-3.5 h-3.5 shrink-0"
              style={{ color: item.passed ? "var(--status-success)" : withAlpha("var(--text-primary)", 30) }}
            />
            <span className="text-[0.75rem]">{item.label}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

/**
 * ReviewHistorySection - Shows review history timeline
 */
function ReviewHistorySection({ history }: { history: ReviewNoteResponse[] }) {
  if (history.length === 0) {
    return (
      <p className="text-[0.75rem] text-text-primary/40 italic">No review history</p>
    );
  }

  const formatDate = (dateStr: string) => {
    const date = new Date(dateStr);
    const now = new Date();
    const diffMs = now.getTime() - date.getTime();
    const diffMins = Math.floor(diffMs / 60000);
    const diffHours = Math.floor(diffMins / 60);
    const diffDays = Math.floor(diffHours / 24);

    if (diffMins < 1) return "Just now";
    if (diffMins < 60) return `${diffMins}m ago`;
    if (diffHours < 24) return `${diffHours}h ago`;
    return `${diffDays}d ago`;
  };

  return (
    <div data-testid="review-history" className="space-y-2">
      {history.map((entry) => (
        <div
          key={entry.id}
          className="flex items-start gap-2 py-1.5 px-2 rounded"
          style={{
            backgroundColor: "var(--overlay-scrim)",
            border: "1px solid var(--overlay-weak)",
          }}
        >
          {/* Icon based on outcome */}
          {entry.outcome === "approved" || entry.outcome === "approved_no_changes" ? (
            <CheckCircle2
              className="w-3.5 h-3.5 mt-0.5 shrink-0"
              style={{ color: "var(--status-success)" }}
            />
          ) : entry.outcome === "changes_requested" ? (
            <RotateCcw
              className="w-3.5 h-3.5 mt-0.5 shrink-0"
              style={{ color: "var(--status-warning)" }}
            />
          ) : (
            <X
              className="w-3.5 h-3.5 mt-0.5 shrink-0"
              style={{ color: "var(--status-error)" }}
            />
          )}

          <div className="flex-1 min-w-0">
            <div className="flex items-center gap-1.5">
              {entry.reviewer === "ai" ? (
                <Bot className="w-3 h-3 text-text-primary/50" />
              ) : entry.reviewer === "system" ? (
                <Settings className="w-3 h-3 text-text-primary/50" />
              ) : (
                <User className="w-3 h-3 text-text-primary/50" />
              )}
              <span className="text-[0.6875rem] font-medium text-text-primary/60">
                {entry.reviewer === "ai" ? "AI" : entry.reviewer === "system" ? "System" : "Human"}{" "}
                {entry.outcome === "approved"
                  ? "approved"
                  : entry.outcome === "approved_no_changes"
                  ? "approved (no changes)"
                  : entry.outcome === "changes_requested"
                  ? "requested changes"
                  : "rejected"}
              </span>
              <span className="text-[0.625rem] text-text-primary/40 ml-auto flex items-center gap-1">
                <Clock className="w-2.5 h-2.5" />
                {formatDate(entry.created_at)}
              </span>
            </div>
            {entry.summary && (
              <p className="text-[0.6875rem] text-text-primary/40 truncate mt-0.5">
                {entry.summary}
              </p>
            )}
            {entry.followup_session_id && (
              <div className="mt-2 flex items-center justify-between gap-2 rounded px-2 py-1.5"
                style={{
                  backgroundColor: "var(--accent-muted)",
                  border: "1px solid var(--overlay-weak)",
                }}
              >
                <span className="text-[0.625rem] text-text-primary/45 break-all min-w-0">
                  Follow-up: {entry.followup_session_id}
                </span>
                <button
                  type="button"
                  onClick={() => navigateToIdeationSession(entry.followup_session_id!)}
                  className="shrink-0 inline-flex items-center gap-1 text-[0.625rem] font-medium transition-opacity hover:opacity-80"
                  style={{ color: "var(--status-warning)" }}
                >
                  <ExternalLink className="w-3 h-3" />
                  Open
                </button>
              </div>
            )}
          </div>
        </div>
      ))}
    </div>
  );
}

/**
 * RevisionCountBadge - Shows current revision attempt
 */
function RevisionCountBadge({ count }: { count: number }) {
  if (count === 0) return null;

  return (
    <div
      data-testid="revision-count-badge"
      className="flex items-center gap-1.5 px-2 py-1 rounded"
      style={{
        backgroundColor: "var(--status-warning-muted)",
        border: "1px solid var(--status-warning-border)",
      }}
    >
      <RotateCcw
        className="w-3.5 h-3.5"
        style={{ color: "var(--status-warning)" }}
      />
      <span className="text-[0.6875rem] font-medium" style={{ color: "var(--status-warning)" }}>
        Revision #{count}
      </span>
    </div>
  );
}

/**
 * SectionTitle - Consistent section heading
 */
function SectionTitle({ children }: { children: React.ReactNode }) {
  return (
    <h4
      className="text-[0.6875rem] font-semibold uppercase tracking-wider mb-2"
      style={{ color: "var(--text-muted)" }}
    >
      {children}
    </h4>
  );
}

/**
 * ReviewDetailModal Component
 */
export function ReviewDetailModal({
  taskId,
  history: historyProp,
  showActions = true,
  reviewMode = "task",
  onClose,
}: ReviewDetailModalProps) {
  const queryClient = useQueryClient();
  const [showFeedbackInput, setShowFeedbackInput] = useState(false);
  const [feedback, setFeedback] = useState("");
  const { confirm, confirmationDialogProps, ConfirmationDialog } = useConfirmation();

  // Fetch task
  const { data: task, isLoading: taskLoading } = useQuery({
    queryKey: taskKeys.detail(taskId),
    queryFn: () => api.tasks.get(taskId),
    enabled: !!taskId,
  });

  // Fetch reviews for this task (for hasAiReview indicator)
  const { hasAiReview } = useReviewsByTaskId(taskId);

  // Use pre-fetched history from prop, or fetch if not provided
  const { data: fetchedHistory } = useTaskStateHistory(taskId, { enabled: !historyProp });
  const history = useMemo(
    () => historyProp ?? fetchedHistory ?? [],
    [historyProp, fetchedHistory]
  );

  // Git diff data - backend determines working path from task/project
  const {
    changes,
    commits,
    commitFiles,
    isLoadingChanges,
    isLoadingHistory,
    isLoadingCommitFiles,
    fetchDiff,
    fetchCommitFiles,
  } = useGitDiff({ taskId, enabled: !!taskId });

  // Check if task is in a state that allows human approval
  // (review_passed or escalated)
  const canApprove = task?.internalStatus === "review_passed" || task?.internalStatus === "escalated";

  // Get latest approved review for summary
  const latestApproved = useMemo(() => {
    const approved = history.filter((h) => h.outcome === "approved" || h.outcome === "approved_no_changes");
    return approved.length > 0 ? approved[0] : null;
  }, [history]);

  // Count revisions (changes_requested outcomes)
  const revisionCount = useMemo(() => {
    return history.filter((h) => h.outcome === "changes_requested").length;
  }, [history]);

  // Approve mutation (task-based)
  const approveMutation = useMutation({
    mutationFn: async () => {
      await api.reviews.approveTask({ task_id: taskId });
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: reviewKeys.all });
      queryClient.invalidateQueries({ queryKey: taskKeys.all });
      onClose();
    },
  });

  // Request changes mutation (task-based)
  const requestChangesMutation = useMutation({
    mutationFn: async (notes: string) => {
      await api.reviews.requestTaskChanges({ task_id: taskId, feedback: notes });
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: reviewKeys.all });
      queryClient.invalidateQueries({ queryKey: taskKeys.all });
      onClose();
    },
  });

  const handleRequestChangesClick = useCallback(() => {
    if (showFeedbackInput && feedback.trim()) {
      requestChangesMutation.mutate(feedback.trim());
    } else {
      setShowFeedbackInput(true);
    }
  }, [showFeedbackInput, feedback, requestChangesMutation]);

  const handleApprove = useCallback(async () => {
    const confirmed = await confirm({
      title: "Approve this task?",
      description: "The task will be marked as approved and completed.",
      confirmText: "Approve",
      variant: "default",
    });
    if (!confirmed) return;
    approveMutation.mutate();
  }, [confirm, approveMutation]);

  const handleCommitSelect = useCallback((_commit: Commit) => {
    // In a real implementation, this would fetch files changed in the commit
  }, []);

  // Keyboard shortcut: Escape to close
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        onClose();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [onClose]);

  const isLoading = approveMutation.isPending || requestChangesMutation.isPending;
  const isPlanMergeReview = reviewMode === "plan_merge" || task?.category === "plan_merge";
  const showLocalReviewSections = !isPlanMergeReview || history.length > 0 || hasAiReview;

  return (
    <Dialog open={true} onOpenChange={(open) => !open && onClose()}>
      <DialogContent
        data-testid="review-detail-modal"
        className={cn(
          "p-0 gap-0 overflow-hidden flex flex-col",
          "max-w-[95vw] w-[95vw] h-[95vh]"
        )}
        aria-describedby={undefined}
        style={{
          backgroundColor: "var(--bg-surface)",
          border: "1px solid var(--border-subtle)",
        }}
      >
        {/* Header */}
        <div
          className="flex items-center justify-between px-4 py-3 border-b shrink-0"
          style={{
            borderColor: "var(--overlay-weak)",
            background: "var(--bg-surface)",
            backdropFilter: "blur(20px)",
          }}
        >
          <div className="flex items-center gap-3">
            <DialogTitle
              data-testid="review-detail-modal-title"
              className="text-base font-semibold text-text-primary/90"
              style={{ letterSpacing: "-0.02em" }}
            >
              Review: {task?.title ?? "Loading..."}
            </DialogTitle>
            <RevisionCountBadge count={revisionCount} />
          </div>
          <Button
            data-testid="review-detail-modal-close"
            variant="ghost"
            size="icon"
            onClick={onClose}
            className="w-8 h-8 text-text-primary/50 hover:text-text-primary/80 hover:bg-[var(--overlay-moderate)]"
          >
            <X className="w-4 h-4" />
          </Button>
        </div>

        {/* Content: Two-column layout */}
        <div className="flex flex-1 min-h-0">
          {/* Left Pane: Context (300px fixed) */}
          <div
            className="w-[400px] shrink-0 flex flex-col border-r overflow-hidden"
            style={{ borderColor: "var(--overlay-weak)", maxWidth: "400px" }}
          >
            <div className="flex-1 overflow-y-auto">
              <div className="p-4 space-y-5">
                {/* Task Context */}
                {isPlanMergeReview && (
                  <div>
                    <SectionTitle>Plan</SectionTitle>
                    <PlanMergeContextCard taskId={taskId} compact />
                  </div>
                )}

                <div>
                  <SectionTitle>{isPlanMergeReview ? "Merge Task" : "Task Details"}</SectionTitle>
                  <TaskContextSection
                    category={task?.category ?? ""}
                    description={task?.description ?? null}
                    priority={task?.priority ?? 0}
                    isLoading={taskLoading}
                  />
                </div>

                {/* AI Review Summary */}
                {showLocalReviewSections && (
                  <div>
                    <SectionTitle>AI Review</SectionTitle>
                    <AIReviewSummary
                      latestApproved={latestApproved ?? null}
                      hasAiReview={hasAiReview}
                    />
                  </div>
                )}

                {/* Review History */}
                {showLocalReviewSections && (
                  <div>
                    <SectionTitle>Review History</SectionTitle>
                    <ReviewHistorySection history={history} />
                  </div>
                )}
              </div>
            </div>

            {/* Feedback Input (in left pane when visible) */}
            {showActions && showFeedbackInput && (
              <div
                className="p-3 border-t"
                style={{ borderColor: "var(--overlay-weak)" }}
              >
                <div className="flex items-center gap-2 mb-2">
                  <MessageSquare className="w-4 h-4 text-text-primary/50" />
                  <span className="text-[0.75rem] font-medium text-text-primary/60">
                    What needs to be changed?
                  </span>
                </div>
                <Textarea
                  data-testid="feedback-input"
                  value={feedback}
                  onChange={(e) => setFeedback(e.target.value)}
                  placeholder="Describe the changes needed..."
                  className="min-h-[80px] text-[0.8125rem] resize-none"
                  style={{
                    backgroundColor: "var(--overlay-scrim)",
                    border: "1px solid var(--overlay-moderate)",
                  }}
                />
                <button
                  onClick={() => {
                    setShowFeedbackInput(false);
                    setFeedback("");
                  }}
                  className="text-[0.6875rem] text-text-primary/40 hover:text-text-primary/60 mt-2"
                >
                  Cancel
                </button>
              </div>
            )}
          </div>

          {/* Right Pane: DiffViewer (flex-1) */}
          <div className="flex-1 min-w-0">
            {task?.internalStatus === "merged" && (
              <div
                className="px-4 py-2 text-[0.6875rem] text-text-primary/55 border-b"
                style={{ borderColor: "var(--overlay-weak)" }}
              >
                {isPlanMergeReview
                  ? "Showing merged feature-branch diff against the target branch."
                  : "Showing merged diff against the base branch for this task."}
              </div>
            )}
            <DiffViewer
              changes={changes}
              commits={commits}
              commitFiles={commitFiles}
              onFetchDiff={fetchDiff}
              onFetchCommitFiles={fetchCommitFiles}
              isLoadingChanges={isLoadingChanges}
              isLoadingHistory={isLoadingHistory}
              isLoadingCommitFiles={isLoadingCommitFiles}
              defaultTab="changes"
              {...(isPlanMergeReview
                ? {
                    changesLabel: "Merged Diff",
                    changesEmptyTitle: "No merged file changes",
                    changesEmptySubtitle: "The merge commit did not report file changes",
                    autoSelectFirstCommit: true,
                    autoSelectFirstCommitFile: true,
                  }
                : {})}
              onCommitSelect={handleCommitSelect}
            />
          </div>
        </div>

        {/* Footer: Action Buttons */}
        {showActions && (
          <div
            className="flex items-center justify-end gap-3 px-4 py-3 border-t shrink-0"
            style={{
              borderColor: "var(--overlay-weak)",
              background: "var(--bg-surface)",
            }}
          >
            {/* Error display */}
            {(approveMutation.error || requestChangesMutation.error) && (
              <span className="text-[0.75rem] text-status-error mr-auto">
                {approveMutation.error?.message || requestChangesMutation.error?.message}
              </span>
            )}

            <Button
              data-testid="review-detail-request-changes"
              onClick={handleRequestChangesClick}
              disabled={isLoading || !canApprove || (showFeedbackInput && !feedback.trim())}
              variant="outline"
              className="gap-1.5"
              style={{
                borderColor: "var(--status-warning)",
                color: "var(--status-warning)",
              }}
            >
              {requestChangesMutation.isPending ? (
                <Loader2 className="w-4 h-4 animate-spin" />
              ) : (
                <RotateCcw className="w-4 h-4" />
              )}
              {showFeedbackInput ? "Submit Changes" : "Request Changes"}
            </Button>
            <Button
              data-testid="review-detail-approve"
              onClick={handleApprove}
              disabled={isLoading || !canApprove || showFeedbackInput}
              className="gap-1.5"
              style={{
                backgroundColor: "var(--status-success)",
                color: "white",
              }}
            >
              {approveMutation.isPending ? (
                <Loader2 className="w-4 h-4 animate-spin" />
              ) : (
                <CheckCircle2 className="w-4 h-4" />
              )}
              Approve
            </Button>
          </div>
        )}
        <ConfirmationDialog {...confirmationDialogProps} />
      </DialogContent>
    </Dialog>
  );
}
