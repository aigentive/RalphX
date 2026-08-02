/**
 * HumanReviewTaskDetail - macOS Tahoe-inspired human review view
 *
 * Shows AI-approved state awaiting human confirmation with premium action buttons.
 */

import { useState, useCallback } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { reviewIssuesApi } from "@/api/review-issues";
import { IssueList, IssueProgressBar } from "@/components/reviews/IssueList";
import type { ReviewIssue, IssueProgressSummary } from "@/types/review-issue";
import { useAgentGate } from "@/hooks/useAgentGate";
import { AgentGateTooltip } from "@/components/remote/AgentGateTooltip";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import {
  SectionTitle,
  DetailCard,
  StatusBanner,
  StatusPill,
  TwoColumnLayout,
} from "./shared";
import { ReviewTimeline } from "./shared/ReviewTimeline";
import { ReviewDetailModal } from "@/components/reviews/ReviewDetailModal";
import { useTaskStateHistory, reviewKeys } from "@/hooks/useReviews";
import { useTaskStateTransitions } from "@/hooks/useTaskStateTransitions";
import { taskKeys } from "@/hooks/useTasks";
import { useConfirmation } from "@/hooks/useConfirmation";
import { api } from "@/lib/tauri";
import { markdownComponents } from "@/components/Chat/MessageItem.markdown";
import {
  Loader2,
  CheckCircle2,
  Bot,
  RotateCcw,
  MessageSquare,
  ShieldCheck,
  ThumbsUp,
  Code,
} from "lucide-react";
import type { Task } from "@/types/task";
import type { ReviewNoteResponse } from "@/lib/tauri";

interface HumanReviewTaskDetailProps {
  task: Task;
  isHistorical?: boolean;
}

/**
 * Get the latest approved review entry
 */
function getLatestApprovedReview(
  history: ReviewNoteResponse[]
): ReviewNoteResponse | null {
  const approvedEntries = history.filter((entry) => entry.outcome === "approved");
  if (approvedEntries.length === 0) return null;
  return approvedEntries[0] ?? null;
}

/**
 * AIReviewCard - Summary of AI review findings with collapsible content
 *
 * Uses issues from reviewIssuesApi and review.notes (clean description)
 */
function AIReviewCard({
  review,
  issues,
  progress,
}: {
  review: ReviewNoteResponse | null;
  issues: ReviewIssue[];
  progress?: IssueProgressSummary;
}) {
  const [isExpanded, setIsExpanded] = useState(false);
  const COLLAPSED_HEIGHT = 120; // pixels

  const summary = review?.notes ?? "";
  const hasContent = summary.length > 100 || issues.length > 0;

  // Click handler - expand when collapsed, clicking anywhere
  const handleCardClick = () => {
    if (!isExpanded && hasContent) {
      setIsExpanded(true);
    }
  };

  return (
    <DetailCard>
      {/* Clickable area when collapsed */}
      <div
        onClick={handleCardClick}
        className={hasContent && !isExpanded ? "cursor-pointer" : ""}
      >
        {/* AI Badge header */}
        <div className="flex items-center gap-3">
          <div
            className="flex items-center justify-center w-9 h-9 rounded-xl shrink-0"
            style={{ backgroundColor: "var(--status-info-muted)" }}
          >
            <Bot className="w-5 h-5" style={{ color: "var(--status-info)" }} />
          </div>
          <div>
            <span className="text-[0.8125rem] font-semibold text-text-primary/80 block">
              AI Review Summary
            </span>
            <span className="text-[0.6875rem] text-text-primary/45">
              {issues.length > 0
                ? `${issues.length} observation${issues.length > 1 ? "s" : ""}`
                : "Automated checks passed"}
            </span>
          </div>
        </div>

        {/* Content area */}
        {(summary || issues.length > 0) && (
          <div className="relative mt-4">
            <div
              className="pl-12 overflow-hidden transition-all duration-300 ease-out"
              style={{
                maxHeight: isExpanded ? "2000px" : `${COLLAPSED_HEIGHT}px`,
              }}
            >
              {/* Issues list using shared IssueList component */}
              {issues.length > 0 && (
                <div className="mb-4">
                  {progress && (
                    <div className="mb-3">
                      <IssueProgressBar progress={progress} />
                    </div>
                  )}
                  <IssueList issues={issues} compact />
                </div>
              )}

              {/* Summary text */}
              {summary && (
                <div className="text-[0.8125rem] text-text-primary/65 leading-relaxed prose prose-sm prose-invert max-w-none">
                  <ReactMarkdown
                    remarkPlugins={[remarkGfm]}
                    components={markdownComponents}
                  >
                    {summary}
                  </ReactMarkdown>
                </div>
              )}
            </div>

            {/* Gradient fade overlay when collapsed */}
            {hasContent && !isExpanded && (
              <div
                className="absolute bottom-0 left-0 right-0 h-12 pointer-events-none"
                style={{
                  background: "linear-gradient(to bottom, color-mix(in srgb, var(--bg-surface) 0%, transparent), var(--bg-surface))",
                }}
              />
            )}
          </div>
        )}

        {/* No content fallback */}
        {!summary && issues.length === 0 && (
          <div className="pl-12 mt-4">
            <div className="flex items-center gap-2 text-[0.8125rem] text-text-primary/50">
              <CheckCircle2 className="w-4 h-4" style={{ color: "var(--status-success)" }} />
              <span>All automated checks passed</span>
            </div>
          </div>
        )}

        {/* Show more - outside gradient, below content */}
        {hasContent && !isExpanded && (
          <div
            className="pl-12 mt-2 text-[0.75rem] font-medium"
            style={{ color: "var(--status-info)" }}
          >
            Show more
          </div>
        )}
      </div>

      {/* Show less button - only when expanded */}
      {hasContent && isExpanded && (
        <button
          onClick={() => setIsExpanded(false)}
          className="pl-12 mt-3 text-[0.75rem] font-medium transition-colors hover:opacity-80"
          style={{ color: "var(--status-info)" }}
        >
          Show less
        </button>
      )}
    </DetailCard>
  );
}

/**
 * ActionButtonsCard - Approve/Request Changes with premium styling
 */
function ActionButtonsCard({
  taskId,
  onReviewCode,
  onApproveSuccess,
  onRequestChangesSuccess,
}: {
  taskId: string;
  onReviewCode?: () => void;
  onApproveSuccess?: () => void;
  onRequestChangesSuccess?: () => void;
}) {
  const queryClient = useQueryClient();
  const [showFeedback, setShowFeedback] = useState(false);
  const [feedback, setFeedback] = useState("");
  const { confirm, confirmationDialogProps, ConfirmationDialog } = useConfirmation();

  const approveMutation = useMutation({
    mutationFn: async () => {
      await api.reviews.approveTask({ task_id: taskId });
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: reviewKeys.all });
      queryClient.invalidateQueries({ queryKey: taskKeys.all });
      onApproveSuccess?.();
    },
  });

  const requestChangesMutation = useMutation({
    mutationFn: async (feedbackText: string) => {
      await api.reviews.requestTaskChanges({ task_id: taskId, feedback: feedbackText });
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: reviewKeys.all });
      queryClient.invalidateQueries({ queryKey: taskKeys.all });
      setShowFeedback(false);
      setFeedback("");
      onRequestChangesSuccess?.();
    },
  });

  const handleRequestChangesClick = () => {
    if (showFeedback && feedback.trim()) {
      requestChangesMutation.mutate(feedback.trim());
    } else {
      setShowFeedback(true);
    }
  };

  const agentGate = useAgentGate("taskApprove");

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

  const isLoading = approveMutation.isPending || requestChangesMutation.isPending;

  return (
    <DetailCard data-testid="action-buttons">
      {/* Feedback input */}
      {showFeedback && (
        <div className="mb-4 space-y-3">
          <div className="flex items-center gap-2">
            <MessageSquare className="w-4 h-4 text-text-primary/40" />
            <span className="text-[0.75rem] font-semibold text-text-primary/60">
              What needs to be changed?
            </span>
          </div>
          <Textarea
            data-testid="feedback-input"
            value={feedback}
            onChange={(e) => setFeedback(e.target.value)}
            placeholder="Describe the changes needed..."
            className="min-h-[100px] text-[0.8125rem] resize-none rounded-xl"
            style={{
              backgroundColor: "var(--overlay-scrim)",
              border: "1px solid var(--overlay-moderate)",
            }}
          />
        </div>
      )}

      {/* Label and action buttons on same row */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <span
            className="text-[0.6875rem] font-semibold uppercase tracking-wider"
            style={{ color: "var(--text-muted)" }}
          >
            Your Decision
          </span>
          {onReviewCode && (
            <Button
              data-testid="review-code-button"
              onClick={onReviewCode}
              variant="ghost"
              className="h-7 px-3 gap-1.5 rounded-lg font-medium text-[0.75rem]"
              style={{ color: "var(--status-info)" }}
            >
              <Code className="w-3.5 h-3.5" />
              Review Code
            </Button>
          )}
        </div>
        <div className="flex gap-2">
        <AgentGateTooltip gated={agentGate.gated} reason={agentGate.reason}><Button
          data-testid="approve-button"
          onClick={handleApprove}
          disabled={isLoading || showFeedback || agentGate.gated}
          className="h-9 px-4 gap-2 rounded-lg font-medium text-[0.8125rem] transition-colors"
          style={{
            backgroundColor: "var(--status-success)",
            color: "white",
          }}
        >
          {approveMutation.isPending ? (
            <Loader2 className="w-4 h-4 animate-spin" />
          ) : (
            <ThumbsUp className="w-4 h-4" />
          )}
          Approve
        </Button></AgentGateTooltip>

        <AgentGateTooltip gated={agentGate.gated} reason={agentGate.reason}><Button
          data-testid="request-changes-button"
          onClick={handleRequestChangesClick}
          disabled={isLoading || (showFeedback && !feedback.trim()) || agentGate.gated}
          variant="ghost"
          className="h-9 px-4 gap-2 rounded-lg font-medium text-[0.8125rem]"
          style={{
            color: "var(--status-warning)",
            backgroundColor: "var(--bg-elevated)",
          }}
        >
          {requestChangesMutation.isPending ? (
            <Loader2 className="w-4 h-4 animate-spin" />
          ) : (
            <RotateCcw className="w-4 h-4" />
          )}
          {showFeedback ? "Submit" : "Request Changes"}
        </Button></AgentGateTooltip>
        </div>
      </div>

      {/* Cancel link */}
      {showFeedback && (
        <button
          onClick={() => {
            setShowFeedback(false);
            setFeedback("");
          }}
          className="mt-3 text-[0.75rem] text-text-primary/40 hover:text-text-primary/60 transition-colors"
        >
          Cancel
        </button>
      )}

      {/* Error display */}
      {(approveMutation.error || requestChangesMutation.error) && (
        <p className="mt-3 text-[0.75rem]" style={{ color: "var(--status-error)" }}>
          {approveMutation.error?.message || requestChangesMutation.error?.message}
        </p>
      )}

      <ConfirmationDialog {...confirmationDialogProps} />
    </DetailCard>
  );
}

export function HumanReviewTaskDetail({ task, isHistorical = false }: HumanReviewTaskDetailProps) {
  const [showReviewModal, setShowReviewModal] = useState(false);
  const { data: history, isLoading } = useTaskStateHistory(task.id);
  const { data: stateTransitions = [] } = useTaskStateTransitions(task.id);
  const latestApprovedReview = getLatestApprovedReview(history);

  // Fetch structured issues from review issues API
  const { data: issues = [] } = useQuery({
    queryKey: ["review-issues", task.id],
    queryFn: () => reviewIssuesApi.getByTaskId(task.id),
  });

  // Fetch issue progress summary
  const { data: progress } = useQuery({
    queryKey: ["issue-progress", task.id],
    queryFn: () => reviewIssuesApi.getProgress(task.id),
  });

  if (isLoading) {
    return (
      <div className="flex items-center justify-center py-16">
        <Loader2
          className="w-6 h-6 animate-spin text-text-primary/30"
        />
      </div>
    );
  }

  return (
    <>
      <TwoColumnLayout
        description={task.description}
        testId="human-review-task-detail"
        actions={
          !isHistorical ? (
            <section>
              <ActionButtonsCard
                taskId={task.id}
                onReviewCode={() => setShowReviewModal(true)}
              />
            </section>
          ) : null
        }
      >
        {/* Status Banner */}
        <StatusBanner
          icon={ShieldCheck}
          title="AI Review Passed"
          subtitle="Awaiting your final approval"
          variant="success"
          badge={
            <StatusPill
              icon={CheckCircle2}
              label="AI Approved"
              variant="success"
              size="md"
            />
          }
        />

        {/* AI Review Summary */}
        <section data-testid="ai-review-summary-section">
          <SectionTitle>AI Review Summary</SectionTitle>
          <AIReviewCard
            review={latestApprovedReview}
            issues={issues}
            {...(progress && { progress })}
          />
        </section>

        {/* Previous Attempts (if any) */}
        {history.filter((e) => e.outcome === "changes_requested").length > 0 && (
          <section data-testid="previous-attempts-section">
            <SectionTitle>Previous Attempts</SectionTitle>
            <ReviewTimeline
              history={history}
              filter={(e) => e.outcome === "changes_requested"}
              showAttemptNumbers
              emptyMessage="No previous attempts"
              stateTransitions={stateTransitions}
            />
          </section>
        )}

      </TwoColumnLayout>

      {/* Review Detail Modal */}
      {showReviewModal && (
        <ReviewDetailModal
          taskId={task.id}
          history={history}
          onClose={() => setShowReviewModal(false)}
        />
      )}
    </>
  );
}
