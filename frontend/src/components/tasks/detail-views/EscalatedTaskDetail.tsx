/**
 * EscalatedTaskDetail - macOS Tahoe-inspired escalation view
 *
 * Shows AI escalation with clear reasoning and actionable decision buttons.
 */

import { useState, useCallback } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { markdownComponents } from "@/components/Chat/MessageItem.markdown";
import { useMutation, useQueryClient, useQuery } from "@tanstack/react-query";
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
import { IssueList } from "@/components/reviews/IssueList";
import { reviewIssuesApi } from "@/api/review-issues";
import { useTaskStateHistory, reviewKeys } from "@/hooks/useReviews";
import { useTaskStateTransitions } from "@/hooks/useTaskStateTransitions";
import { taskKeys } from "@/hooks/useTasks";
import { useConfirmation } from "@/hooks/useConfirmation";
import { api } from "@/lib/tauri";
import { navigateToIdeationSession } from "@/lib/navigation";
import { statusTint } from "@/lib/theme-colors";
import {
  Loader2,
  AlertTriangle,
  Bot,
  RotateCcw,
  MessageSquare,
  HelpCircle,
  ThumbsUp,
  RefreshCw,
  XCircle,
  Clock,
  Settings,
  ExternalLink,
} from "lucide-react";
import type { Task } from "@/types/task";
import type { ReviewNoteResponse } from "@/lib/tauri";

interface EscalatedTaskDetailProps {
  task: Task;
  isHistorical?: boolean;
}

function getLatestEscalationReview(
  history: ReviewNoteResponse[]
): ReviewNoteResponse | null {
  if (history.length === 0) return null;
  return history[0] ?? null;
}

type EscalationType = "system_interrupted" | "reason_provided" | "agent_error" | "incomplete" | "failed";

interface EscalationInfo {
  type: EscalationType;
  header: string;
  subtitle: string;
  reason: string | null;
  suggestReReview: boolean;
}

function parseMetadataError(metadata: string | null | undefined): string | null {
  if (!metadata) return null;
  try {
    const obj = JSON.parse(metadata) as Record<string, unknown>;
    const err = obj["last_agent_error"];
    return typeof err === "string" ? err : null;
  } catch {
    return null;
  }
}

function determineEscalationInfo(
  review: ReviewNoteResponse | null,
  metadata: string | null | undefined
): EscalationInfo {
  const agentError = parseMetadataError(metadata);
  const notes = review?.notes ?? null;

  // Primary signal: system reviewer type means agent was killed, not an AI decision
  if (review?.reviewer === "system") {
    return {
      type: "system_interrupted",
      header: "Review Interrupted",
      subtitle: "Review was interrupted (agent stopped or app restarted)",
      reason: notes ?? "The review agent was interrupted before completing its review.",
      suggestReReview: true,
    };
  }

  if (notes) {
    const notesLower = notes.toLowerCase();
    const isTimeout = notesLower.includes("timed out");
    const isFailed = /failed \d+ times/.test(notesLower);
    if (isTimeout || isFailed) {
      return {
        type: "failed",
        header: "Review Failed",
        subtitle: "The reviewer couldn't complete after multiple attempts",
        reason: notes,
        suggestReReview: false,
      };
    }
    return {
      type: "reason_provided",
      header: "Escalated to Human",
      subtitle: "AI reviewer couldn't make a decision",
      reason: notes,
      suggestReReview: false,
    };
  }

  if (agentError) {
    return {
      type: "agent_error",
      header: "Review Interrupted",
      subtitle: "Review agent encountered an error",
      reason: `Review agent encountered an error: ${agentError}`,
      suggestReReview: true,
    };
  }

  return {
    type: "incomplete",
    header: "Review Incomplete",
    subtitle: "Review was interrupted before completion",
    reason: "The review was interrupted before completion (app restart or timeout)",
    suggestReReview: true,
  };
}

const ESCALATION_ICON_MAP: Record<EscalationType, React.ElementType> = {
  system_interrupted: Settings,
  reason_provided: Bot,
  agent_error: XCircle,
  incomplete: Clock,
  failed: AlertTriangle,
};

/**
 * EscalationReasonCard - Shows why AI escalated (reason text only)
 */
function EscalationReasonCard({
  escalationInfo,
}: {
  escalationInfo: EscalationInfo;
}) {
  const Icon = ESCALATION_ICON_MAP[escalationInfo.type];
  return (
    <DetailCard variant="warning">
      {/* Header */}
      <div className="flex items-center gap-3 mb-4">
        <div
          className="flex items-center justify-center w-9 h-9 rounded-xl shrink-0"
          style={{ backgroundColor: "var(--status-warning-border)" }}
        >
          <Icon className="w-5 h-5" style={{ color: "var(--status-warning)" }} />
        </div>
        <div>
          <span className="text-[0.8125rem] font-semibold text-text-primary/80 block">
            Escalation Reason
          </span>
          <span className="text-[0.6875rem] text-text-primary/45">
            {escalationInfo.subtitle}
          </span>
        </div>
      </div>

      {/* Reason text */}
      {escalationInfo.reason ? (
        <div
          className="text-[0.8125rem] text-text-primary/55 leading-relaxed pl-12"
          style={{ wordBreak: "break-word" }}
        >
          <ReactMarkdown remarkPlugins={[remarkGfm]} components={markdownComponents}>
            {escalationInfo.reason}
          </ReactMarkdown>
        </div>
      ) : (
        <p className="text-[0.8125rem] text-text-primary/35 italic pl-12">
          No escalation reason provided
        </p>
      )}
    </DetailCard>
  );
}

/**
 * DecisionButtonsCard - Human decision actions
 */
function DecisionButtonsCard({
  taskId,
  suggestReReview,
  onApproveSuccess,
  onRequestChangesSuccess,
  onReReviewSuccess,
}: {
  taskId: string;
  suggestReReview: boolean;
  onApproveSuccess?: () => void;
  onRequestChangesSuccess?: () => void;
  onReReviewSuccess?: () => void;
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

  const reReviewMutation = useMutation({
    mutationFn: async () => {
      await api.reviews.reReviewTask({ task_id: taskId });
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: reviewKeys.all });
      queryClient.invalidateQueries({ queryKey: taskKeys.all });
      onReReviewSuccess?.();
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
      title: "Approve despite concerns?",
      description: "The AI flagged potential issues. Are you sure you want to approve?",
      confirmText: "Approve Anyway",
      variant: "default",
    });
    if (!confirmed) return;
    approveMutation.mutate();
  }, [confirm, approveMutation]);

  const isLoading =
    approveMutation.isPending ||
    requestChangesMutation.isPending ||
    reReviewMutation.isPending;

  const reReviewButton = (
    <Button
      data-testid="re-review-button"
      onClick={() => reReviewMutation.mutate()}
      disabled={isLoading || showFeedback}
      className="h-9 px-4 gap-2 rounded-lg font-medium text-[0.8125rem] transition-colors"
      style={{
        backgroundColor: "var(--accent-primary)",
        color: "white",
      }}
    >
      {reReviewMutation.isPending ? (
        <Loader2 className="w-4 h-4 animate-spin" />
      ) : (
        <RefreshCw className="w-4 h-4" />
      )}
      Re-Review
    </Button>
  );

  const approveButton = (
    <AgentGateTooltip
      gated={agentGate.gated}
      reason={agentGate.reason}
      testId="approve-button-gate"
    >
    <Button
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
      Approve Anyway
    </Button>
    </AgentGateTooltip>
  );

  const requestChangesButton = (
    <Button
      data-testid="request-changes-button"
      onClick={handleRequestChangesClick}
      disabled={isLoading || (showFeedback && !feedback.trim())}
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
    </Button>
  );

  return (
    <DetailCard>
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

      {/* Decision buttons — re-review first when suggested */}
      <div className="flex gap-2 justify-end">
        {suggestReReview ? (
          <>
            {reReviewButton}
            {approveButton}
            {requestChangesButton}
          </>
        ) : (
          <>
            {approveButton}
            {requestChangesButton}
            {reReviewButton}
          </>
        )}
      </div>

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

      {(approveMutation.error ||
        requestChangesMutation.error ||
        reReviewMutation.error) && (
        <p className="mt-3 text-[0.75rem]" style={{ color: "var(--status-error)" }}>
          {approveMutation.error?.message ||
            requestChangesMutation.error?.message ||
            reReviewMutation.error?.message}
        </p>
      )}

      <ConfirmationDialog {...confirmationDialogProps} />
    </DetailCard>
  );
}

export function EscalatedTaskDetail({ task, isHistorical = false }: EscalatedTaskDetailProps) {
  const { data: history, isLoading } = useTaskStateHistory(task.id);
  const { data: stateTransitions = [] } = useTaskStateTransitions(task.id);
  const escalationReview = getLatestEscalationReview(history ?? []);
  const escalationInfo = determineEscalationInfo(escalationReview, task.metadata);
  const followupSessionId = escalationReview?.followup_session_id ?? null;

  // Fetch structured issues from review issues API
  const { data: issues = [] } = useQuery({
    queryKey: ["review-issues", task.id],
    queryFn: () => reviewIssuesApi.getByTaskId(task.id),
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
    <TwoColumnLayout
      description={task.description}
      testId="escalated-task-detail"
    >
      {/* Status Banner */}
      <StatusBanner
        icon={HelpCircle}
        title={escalationInfo.header}
        subtitle={escalationInfo.subtitle}
        variant="warning"
        badge={
          <StatusPill
            icon={AlertTriangle}
            label="Needs Decision"
            variant="warning"
            size="md"
          />
        }
      />

      {/* Escalation Reason */}
      <section data-testid="ai-escalation-reason-section">
        <SectionTitle>Why AI Escalated</SectionTitle>
        <EscalationReasonCard escalationInfo={escalationInfo} />
      </section>

      {followupSessionId && (
        <section data-testid="followup-session-section">
          <SectionTitle>Follow-Up Session</SectionTitle>
          <DetailCard>
            <div className="flex items-center justify-between gap-3">
              <div className="min-w-0">
                <div className="text-[0.8125rem] font-semibold text-text-primary/80">
                  AI spawned a follow-up ideation session
                </div>
                <div className="mt-1 text-[0.75rem] text-text-primary/45 break-all">
                  {followupSessionId}
                </div>
              </div>
              <Button
                type="button"
                onClick={() => navigateToIdeationSession(followupSessionId)}
                className="h-9 px-4 gap-2 rounded-lg font-medium text-[0.8125rem]"
                style={{
                  backgroundColor: statusTint("accent", 16),
                  color: "var(--accent-primary)",
                }}
              >
                <ExternalLink className="w-4 h-4" />
                Open Follow-Up
              </Button>
            </div>
          </DetailCard>
        </section>
      )}

      {/* Issues Found */}
      {issues.length > 0 && (
        <section data-testid="issues-section">
          <SectionTitle>Issues Found ({issues.length})</SectionTitle>
          <IssueList issues={issues} groupBy="severity" compact />
        </section>
      )}

      {/* Previous Attempts */}
      <section data-testid="previous-attempts-section">
        <SectionTitle>Previous Attempts</SectionTitle>
        <ReviewTimeline
          history={history ?? []}
          filter={(e) => e.outcome === "changes_requested"}
          showAttemptNumbers
          emptyMessage="No previous attempts"
          stateTransitions={stateTransitions}
        />
      </section>

      {/* Decision Buttons (hidden in historical mode) */}
      {!isHistorical && (
        <section data-testid="action-buttons">
          <SectionTitle>Your Decision</SectionTitle>
          <DecisionButtonsCard
            taskId={task.id}
            suggestReReview={escalationInfo.suggestReReview}
          />
        </section>
      )}
    </TwoColumnLayout>
  );
}
