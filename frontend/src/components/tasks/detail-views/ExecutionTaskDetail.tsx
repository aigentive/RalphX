/**
 * ExecutionTaskDetail - macOS Tahoe-inspired execution view
 *
 * Live execution state with animated progress, step tracking,
 * and revision context when re-executing.
 */

import { useCallback, useMemo, useState } from "react";
import { Loader2, Radio, AlertTriangle, Bot, User, Settings, Zap, MoreVertical, Square, Ban } from "lucide-react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { StepList } from "../StepList";
import {
  SectionTitle,
  DetailCard,
  StatusBanner,
  StatusPill,
  ProgressIndicator,
  TwoColumnLayout,
} from "./shared";
import { ValidationProgress } from "./shared/ValidationProgress";
import { DurationDisplay } from "./shared/DurationDisplay";
import { useTaskSteps, useStepProgress } from "@/hooks/useTaskSteps";
import { useTaskStateHistory } from "@/hooks/useReviews";
import { reviewIssuesApi } from "@/api/review-issues";
import { IssueList } from "@/components/reviews/IssueList";
import { useConfirmation } from "@/hooks/useConfirmation";
import { api } from "@/lib/tauri";
import { taskKeys } from "@/hooks/useTasks";
import {
  DropdownMenu,
  DropdownMenuTrigger,
  DropdownMenuContent,
  DropdownMenuItem,
} from "@/components/ui/dropdown-menu";
import { Button } from "@/components/ui/button";
import type { Task } from "@/types/task";
import type { ReviewNoteResponse } from "@/lib/tauri";
import { useValidationEvents } from "@/hooks/useValidationEvents";
import { ReviewFeedbackBody } from "@/components/reviews/ReviewFeedbackBody";
import { getReviewFeedbackHeading } from "@/lib/review-feedback";
import { getStepProgressDisplay } from "@/types/task-step";
import { useAgentGate } from "@/hooks/useAgentGate";
import { AgentGateTooltip } from "@/components/remote/AgentGateTooltip";

interface ExecutionTaskDetailProps {
  task: Task;
  isHistorical?: boolean;
}

/**
 * ActionButtonsCard - Stop/Cancel actions for stuck execution tasks
 */
function ActionButtonsCard({
  taskId,
  isProcessing,
  onActionSuccess,
}: {
  taskId: string;
  isProcessing: boolean;
  onActionSuccess?: () => void;
}) {
  const queryClient = useQueryClient();
  const { confirm, confirmationDialogProps, ConfirmationDialog } = useConfirmation();
  const [error, setError] = useState<string | null>(null);
  const stopGate = useAgentGate("taskStop");
  const moveGate = useAgentGate("taskMove");

  const stopMutation = useMutation({
    mutationFn: async () => {
      await api.tasks.stop(taskId);
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: taskKeys.all });
      onActionSuccess?.();
      setError(null);
    },
    onError: (err) => {
      setError(err instanceof Error ? err.message : "Failed to stop task");
    },
  });

  const cancelMutation = useMutation({
    mutationFn: async () => {
      await api.tasks.move(taskId, "cancelled");
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: taskKeys.all });
      onActionSuccess?.();
      setError(null);
    },
    onError: (err) => {
      setError(err instanceof Error ? err.message : "Failed to cancel task");
    },
  });

  const handleStop = useCallback(async () => {
    const confirmed = await confirm({
      title: "Stop this task?",
      description:
        "This will permanently stop the task. You can restart it from the Ready state.",
      confirmText: "Stop Task",
      variant: "destructive",
    });
    if (!confirmed) return;
    stopMutation.mutate();
  }, [confirm, stopMutation]);

  const handleCancel = useCallback(async () => {
    const confirmed = await confirm({
      title: "Cancel this task?",
      description: "This will cancel the task and move it to the Cancelled state.",
      confirmText: "Cancel Task",
      variant: "destructive",
    });
    if (!confirmed) return;
    cancelMutation.mutate();
  }, [confirm, cancelMutation]);

  const isLoading = isProcessing || stopMutation.isPending || cancelMutation.isPending;

  return (
    <>
      <DetailCard>
        <div className="flex items-center justify-between">
          <span
            className="text-[0.6875rem] font-semibold uppercase tracking-wider"
            style={{ color: "var(--text-muted)" }}
          >
            Actions
          </span>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button
                data-testid="action-dropdown-trigger"
                variant="ghost"
                size="sm"
                className="h-8 w-8 p-0"
              >
                <MoreVertical className="w-4 h-4" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <AgentGateTooltip gated={stopGate.gated} reason={stopGate.reason} className="flex"><DropdownMenuItem
                data-testid="stop-action"
                onClick={handleStop}
                disabled={isLoading || stopGate.gated}
              >
                <Square className="w-4 h-4 mr-2" />
                <span>Stop</span>
              </DropdownMenuItem></AgentGateTooltip>
              <AgentGateTooltip gated={moveGate.gated} reason={moveGate.reason} className="flex"><DropdownMenuItem
                data-testid="cancel-action"
                onClick={handleCancel}
                disabled={isLoading || moveGate.gated}
              >
                <Ban className="w-4 h-4 mr-2" />
                <span>Cancel</span>
              </DropdownMenuItem></AgentGateTooltip>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>

        {error && (
          <p className="mt-3 text-[0.75rem]" style={{ color: "var(--status-error)" }}>
            {error}
          </p>
        )}
      </DetailCard>
      <ConfirmationDialog {...confirmationDialogProps} />
    </>
  );
}

/**
 * Get the latest revision feedback from history
 */
function getLatestRevisionFeedback(
  history: ReviewNoteResponse[]
): ReviewNoteResponse | null {
  const revisionEntries = history.filter(
    (entry) => entry.outcome === "changes_requested"
  );
  if (revisionEntries.length === 0) return null;
  return revisionEntries[0] ?? null;
}

/**
 * RevisionFeedbackCard - Shows the feedback being addressed during re-execution
 */
function RevisionFeedbackCard({
  feedback,
  isLoading,
}: {
  feedback: ReviewNoteResponse | null;
  isLoading: boolean;
}) {
  if (isLoading) {
    return (
      <div className="flex items-center justify-center py-6">
        <Loader2
          className="w-5 h-5 animate-spin text-text-primary/30"
        />
      </div>
    );
  }

  if (!feedback) return null;

  const isAiReviewer = feedback.reviewer === "ai";
  const isSystemReviewer = feedback.reviewer === "system";

  return (
    <DetailCard variant="warning">
      <div className="flex items-start gap-3">
        {/* Reviewer icon */}
        <div
          className="flex items-center justify-center w-8 h-8 rounded-xl shrink-0"
          style={{
            backgroundColor: isAiReviewer
              ? "var(--status-info-muted)"
              : isSystemReviewer
              ? "var(--status-warning-muted)"
              : "var(--status-success-muted)",
          }}
        >
          {isAiReviewer ? (
            <Bot className="w-4 h-4" style={{ color: "var(--status-info)" }} />
          ) : isSystemReviewer ? (
            <Settings className="w-4 h-4" style={{ color: "var(--status-warning)" }} />
          ) : (
            <User className="w-4 h-4" style={{ color: "var(--status-success)" }} />
          )}
        </div>

        {/* Feedback content */}
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2 mb-1">
            <span className="text-[0.75rem] font-semibold text-text-primary/70">
              {getReviewFeedbackHeading(feedback.reviewer, true)}
            </span>
            <StatusPill
              icon={AlertTriangle}
              label="Addressing"
              variant="warning"
              size="sm"
            />
          </div>
          <div className="text-[0.8125rem] text-text-primary/55 leading-relaxed" style={{ wordBreak: "break-word" }}>
            <ReviewFeedbackBody
              summary={feedback.summary ?? null}
              notes={feedback.notes ?? null}
              dialogTitle="Full revision feedback"
              dialogDescription="Full review feedback in a scrollable view."
              fullButtonLabel="View full feedback"
              previewClassName="text-[0.8125rem] text-text-primary/55 leading-relaxed"
            />
          </div>
        </div>
      </div>
    </DetailCard>
  );
}

export function ExecutionTaskDetail({ task, isHistorical }: ExecutionTaskDetailProps) {
  const { data: steps, isLoading: stepsLoading } = useTaskSteps(task.id);
  const { data: progress } = useStepProgress(task.id, { isExecuting: !isHistorical });
  const { data: history, isLoading: historyLoading } = useTaskStateHistory(
    task.id,
    { enabled: task.internalStatus === "re_executing", refetchOnMount: true }
  );

  // Fetch open issues when re-executing to show what needs to be addressed
  const { data: openIssues = [] } = useQuery({
    queryKey: ["review-issues", task.id, "open"],
    queryFn: () => reviewIssuesApi.getByTaskId(task.id, "open"),
    enabled: task.internalStatus === "re_executing",
  });

  // Live validation events for setup/install progress
  const liveValidationSteps = useValidationEvents(task.id, "execution");

  const hasSteps = (steps?.length ?? 0) > 0;
  const isReExecuting = task.internalStatus === "re_executing";
  const revisionFeedback = isReExecuting
    ? getLatestRevisionFeedback(history ?? [])
    : null;

  // Parse last_agent_error from metadata for historical view
  const agentError = useMemo(() => {
    if (!task.metadata) return null;
    try {
      const metadata = JSON.parse(task.metadata);
      const lastError = metadata.last_agent_error;
      if (!lastError) return null;
      const errorContext: string | undefined = metadata.last_agent_error_context;
      const failureSource: string | undefined = metadata.failure_source;
      const contextLabel =
        failureSource === "validation_failed" ? "Validation"
        : failureSource === "local_tool_failed" ? "Local Tool"
        : errorContext === "review" ? "Reviewer"
        : errorContext === "execution" ? "Worker"
        : "Agent";
      const title =
        failureSource === "validation_failed" ? "Validation Failed"
        : failureSource === "local_tool_failed" ? "Local Tool Failed"
        : `${contextLabel} Error`;
      return {
        message: lastError as string,
        contextLabel,
        title,
        errorAt: metadata.last_agent_error_at as string | undefined,
      };
    } catch {
      return null;
    }
  }, [task.metadata]);

  const rawTotal = progress?.total ?? 0;
  const progressDisplay = progress
    ? getStepProgressDisplay(progress)
    : { completed: 0, total: 0, completedPercent: 0, activePercent: 0 };
  const completed = progressDisplay.completed;
  const total = progressDisplay.total;
  const shouldAnimateActiveProgress =
    !isHistorical &&
    (progress?.inProgress ?? 0) > 0 &&
    progressDisplay.activePercent > progressDisplay.completedPercent;

  return (
    <TwoColumnLayout
      description={task.description}
      testId="execution-task-detail"
    >
      {/* Status Banner */}
      <StatusBanner
        icon={isHistorical ? Zap : Radio}
        title={isHistorical ? "Execution Completed" : isReExecuting ? "Revising Task" : "Executing Task"}
        subtitle={isHistorical ? "This execution has finished" : "AI agent is actively working"}
        variant={isHistorical ? "success" : isReExecuting ? "warning" : "accent"}
        animated={!isHistorical}
        badge={
          <StatusPill
            icon={isHistorical ? Zap : Radio}
            label={isHistorical ? "Done" : isReExecuting ? "Revising" : "Live"}
            variant={isHistorical ? "success" : isReExecuting ? "warning" : "accent"}
            animated={!isHistorical}
            size="md"
          />
        }
      />

      {/* Duration — live ticker while executing, static once done */}
      {task.startedAt && (
        <div data-testid="execution-duration">
          <DurationDisplay
            mode={isHistorical ? "static" : "live"}
            startedAt={task.startedAt}
            completedAt={isHistorical ? task.completedAt : null}
          />
        </div>
      )}

      {/* Agent Error Banner - shows last_agent_error in historical mode */}
      {isHistorical && agentError && (
        <section data-testid="agent-error-section" className="space-y-2">
          <SectionTitle>{agentError.title}</SectionTitle>
          <DetailCard variant="warning">
            <div className="flex items-start gap-2.5">
              <AlertTriangle
                className="w-4 h-4 mt-0.5 shrink-0"
                style={{ color: "var(--status-warning)" }}
              />
              <div className="flex-1 min-w-0">
                <p className="text-[0.8125rem]" style={{ color: "var(--status-warning)" }}>
                  {agentError.message}
                </p>
                {agentError.errorAt && (
                  <span
                    className="text-[0.6875rem] mt-1.5 block"
                    style={{ color: "var(--text-muted)" }}
                  >
                    {new Date(agentError.errorAt).toLocaleString()}
                  </span>
                )}
              </div>
            </div>
          </DetailCard>
        </section>
      )}

      {/* Progress Section */}
      {rawTotal > 0 && total > 0 && (
        <section data-testid="execution-progress-section">
          <SectionTitle>Progress</SectionTitle>
          <DetailCard>
            <ProgressIndicator
              percentComplete={progressDisplay.completedPercent}
              activePercentComplete={progressDisplay.activePercent}
              animateActiveProgress={shouldAnimateActiveProgress}
              completedSteps={completed}
              totalSteps={total}
              variant={isReExecuting ? "info" : "accent"}
            />
          </DetailCard>
        </section>
      )}


      {/* Setup/Install Progress (live validation events) */}
      <ValidationProgress
        taskId={task.id}
        metadata={task.metadata}
        liveSteps={liveValidationSteps}
        title="Setup & Install"
        metadataLogKey="execution_setup_log"
      />

      {/* Revision Feedback (only for re-executing with feedback or while loading) */}
      {isReExecuting && (revisionFeedback !== null || historyLoading) && (
        <section data-testid="revision-feedback-banner">
          <SectionTitle>Feedback Being Addressed</SectionTitle>
          <RevisionFeedbackCard
            feedback={revisionFeedback}
            isLoading={historyLoading}
          />
        </section>
      )}

      {/* Open Issues to Address (only for re-executing with issues) */}
      {isReExecuting && openIssues.length > 0 && (
        <section data-testid="open-issues-section">
          <SectionTitle>Issues to Address ({openIssues.length})</SectionTitle>
          <IssueList issues={openIssues} groupBy="severity" compact />
        </section>
      )}

      {/* Steps Section */}
      {stepsLoading && (
        <div
          data-testid="execution-steps-loading"
          className="flex items-center justify-center py-8"
        >
          <Loader2
            className="w-5 h-5 animate-spin text-text-primary/30"
          />
        </div>
      )}

      {!stepsLoading && hasSteps && (
        <section data-testid="execution-steps-section">
          <SectionTitle>Steps</SectionTitle>
          <StepList taskId={task.id} editable={false} />
        </section>
      )}

      {/* Action Buttons (hidden in historical mode) */}
      {!isHistorical && (
        <section data-testid="action-buttons-section">
          <ActionButtonsCard
            taskId={task.id}
            isProcessing={false}
          />
        </section>
      )}
    </TwoColumnLayout>
  );
}
