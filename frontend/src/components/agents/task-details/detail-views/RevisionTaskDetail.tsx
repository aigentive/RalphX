/**
 * RevisionTaskDetail - macOS Tahoe-inspired revision needed view
 *
 * Shows the revision feedback and attempt count with native styling.
 */

import { StepList } from "../StepList";
import {
  SectionTitle,
  DetailCard,
  StatusBanner,
  StatusPill,
  TwoColumnLayout,
} from "./shared";
import { useTaskSteps } from "@/hooks/useTaskSteps";
import { useTaskStateHistory } from "@/hooks/useReviews";
import { DurationDisplay } from "./shared/DurationDisplay";
import { useQuery } from "@tanstack/react-query";
import { reviewIssuesApi } from "@/api/review-issues";
import { IssueList } from "@/components/reviews/IssueList";
import {
  Loader2,
  AlertTriangle,
  Bot,
  User,
  Settings,
  RotateCcw,
  MessageCircleWarning,
} from "lucide-react";
import type { Task } from "@/types/task";
import type { ReviewNoteResponse } from "@/lib/tauri";
import type { ReviewIssue } from "@/types/review-issue";
import { ReviewFeedbackBody } from "@/components/reviews/ReviewFeedbackBody";
import { getReviewFeedbackHeading } from "@/lib/review-feedback";

interface RevisionTaskDetailProps {
  task: Task;
}

function formatTimeAgo(isoDate: string): string {
  const diff = Date.now() - new Date(isoDate).getTime();
  const mins = Math.floor(diff / 60000);
  if (mins < 1) return "Just now";
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  return `${days}d ago`;
}

function calculateAttemptNumber(history: ReviewNoteResponse[]): number {
  const changesRequestedCount = history.filter(
    (entry) => entry.outcome === "changes_requested"
  ).length;
  return changesRequestedCount + 1;
}

function getLatestRevisionFeedback(
  history: ReviewNoteResponse[]
): ReviewNoteResponse | null {
  const revisionEntries = history.filter(
    (entry) => entry.outcome === "changes_requested"
  );
  if (revisionEntries.length === 0) return null;
  return revisionEntries[0] ?? null;
}

interface FeedbackCardProps {
  review: ReviewNoteResponse;
  issues: ReviewIssue[];
}

/**
 * FeedbackCard - Shows the review feedback that triggered revision
 */
function FeedbackCard({ review, issues }: FeedbackCardProps) {
  const isAiReviewer = review.reviewer === "ai";
  const isSystemReviewer = review.reviewer === "system";
  const timeAgo = formatTimeAgo(review.created_at);

  return (
    <DetailCard variant="warning">
      {/* Header */}
      <div className="flex items-center gap-3 mb-4">
        <div
          className="flex items-center justify-center w-9 h-9 rounded-xl shrink-0"
          style={{
            backgroundColor: isAiReviewer
              ? "var(--status-info-muted)"
              : isSystemReviewer
              ? "var(--status-warning-muted)"
              : "var(--status-success-muted)",
          }}
        >
          {isAiReviewer ? (
            <Bot className="w-5 h-5" style={{ color: "var(--status-info)" }} />
          ) : isSystemReviewer ? (
            <Settings className="w-5 h-5" style={{ color: "var(--status-warning)" }} />
          ) : (
            <User className="w-5 h-5" style={{ color: "var(--status-success)" }} />
          )}
        </div>
        <div className="flex-1">
          <span className="text-[0.8125rem] font-semibold text-text-primary/80 block">
            {getReviewFeedbackHeading(review.reviewer)}
          </span>
          <span className="text-[0.6875rem] text-text-primary/40">{timeAgo}</span>
        </div>
      </div>

      {/* Main feedback text (show notes only if no structured issues) */}
      {issues.length === 0 && (review.summary || review.notes) && (
        <div className="text-[0.8125rem] text-text-primary/55 leading-relaxed mb-4 pl-12" style={{ wordBreak: "break-word" }}>
          <ReviewFeedbackBody
            summary={review.summary ?? null}
            notes={review.notes ?? null}
            dialogTitle="Full revision feedback"
            dialogDescription="Full review feedback in a scrollable view."
            fullButtonLabel="View full feedback"
            previewClassName="text-[0.8125rem] text-text-primary/55 leading-relaxed"
          />
        </div>
      )}

      {/* Structured Issues list */}
      {issues.length > 0 && (
        <div className="pl-12">
          <div className="flex items-center gap-2 mb-2">
            <AlertTriangle className="w-3.5 h-3.5" style={{ color: "var(--status-warning)" }} />
            <span className="text-[0.6875rem] font-semibold uppercase tracking-wider text-text-primary/50">
              Issues to Address
            </span>
          </div>
          <IssueList issues={issues} groupBy="status" compact />
        </div>
      )}
    </DetailCard>
  );
}

export function RevisionTaskDetail({ task }: RevisionTaskDetailProps) {
  const { data: steps, isLoading: stepsLoading } = useTaskSteps(task.id);
  const { data: history, isLoading: historyLoading } = useTaskStateHistory(task.id);
  const { data: issues = [] } = useQuery({
    queryKey: ["review-issues", task.id],
    queryFn: () => reviewIssuesApi.getByTaskId(task.id),
  });
  const hasSteps = (steps?.length ?? 0) > 0;

  const attemptNumber = calculateAttemptNumber(history);
  const latestFeedback = getLatestRevisionFeedback(history);

  return (
    <TwoColumnLayout
      description={task.description}
      testId="revision-task-detail"
    >
      {/* Status Banner */}
      <StatusBanner
        icon={MessageCircleWarning}
        title="Revision Needed"
        subtitle="Changes were requested by the reviewer"
        variant="warning"
        badge={
          <StatusPill
            icon={RotateCcw}
            label={`Attempt #${attemptNumber}`}
            variant="warning"
            size="md"
          />
        }
      />

      {/* Duration (static — shows elapsed from start) */}
      {task.startedAt && (
        <div data-testid="revision-task-duration">
          <DurationDisplay
            mode="static"
            startedAt={task.startedAt}
            completedAt={task.completedAt}
          />
        </div>
      )}

      {/* Review Feedback */}
      {historyLoading ? (
        <div className="flex items-center justify-center py-8">
          <Loader2
            className="w-5 h-5 animate-spin text-text-primary/30"
          />
        </div>
      ) : latestFeedback ? (
        <section data-testid="revision-feedback-section">
          <SectionTitle>Feedback to Address</SectionTitle>
          <FeedbackCard review={latestFeedback} issues={issues} />
        </section>
      ) : null}

      {/* Steps */}
      {stepsLoading && (
        <div
          data-testid="revision-steps-loading"
          className="flex items-center justify-center py-8"
        >
          <Loader2
            className="w-5 h-5 animate-spin text-text-primary/30"
          />
        </div>
      )}

      {!stepsLoading && hasSteps && (
        <section data-testid="revision-steps-section">
          <SectionTitle>Steps</SectionTitle>
          <StepList taskId={task.id} editable={false} />
        </section>
      )}
    </TwoColumnLayout>
  );
}
