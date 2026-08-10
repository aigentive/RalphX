/**
 * ReviewingTaskDetail - macOS Tahoe-inspired AI review in progress view
 *
 * Shows animated review progress with step indicator and clean layout.
 */

import { useState, useCallback } from "react";
import {
  Loader2,
  Bot,
  CheckCircle2,
  Circle,
  Sparkles,
  AlertTriangle,
  XCircle,
  Square,
  RotateCcw,
} from "lucide-react";
import { Textarea } from "@/components/ui/textarea";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  SectionTitle,
  DetailCard,
  StatusBanner,
  StatusPill,
  TwoColumnLayout,
  TaskValidationSection,
} from "./shared";
import { ValidationProgress } from "./shared/ValidationProgress";
import { DurationDisplay } from "./shared/DurationDisplay";
import { useConfirmation } from "@/hooks/useConfirmation";
import { api } from "@/lib/tauri";
import { taskKeys } from "@/hooks/useTasks";
import type { Task } from "@/types/task";
import { useTaskStateHistory } from "@/hooks/useReviews";
import { useValidationEvents } from "@/hooks/useValidationEvents";
import type { ReviewNoteResponse } from "@/lib/tauri";
import { statusTint, withAlpha } from "@/lib/theme-colors";
import { useAgentGate } from "@/hooks/useAgentGate";
import { AgentGateTooltip } from "@/components/remote/AgentGateTooltip";

interface ReviewingTaskDetailProps {
  task: Task;
  isHistorical?: boolean;
  viewTimestamp?: string | undefined;
}

type ReviewStepStatus = "completed" | "active" | "pending";

interface ReviewStep {
  label: string;
  status: ReviewStepStatus;
  isLast?: boolean;
}

/**
 * ReviewStepItem - Individual step with native-feeling progress indicator
 */
function ReviewStepItem({
  label,
  status,
  isHistorical,
  isLast = false,
}: ReviewStep & { isHistorical?: boolean }) {
  return (
    <div
      className="flex items-center gap-3 px-3 py-3"
      style={
        !isLast
          ? { borderBottom: "1px solid var(--border-subtle)" }
          : undefined
      }
    >
      {/* Status icon */}
      <div className="relative">
        {status === "completed" && (
          <CheckCircle2 className="w-5 h-5" style={{ color: "var(--status-success)" }} />
        )}
        {status === "active" && !isHistorical && (
          <div className="relative">
            <Loader2
              className="w-5 h-5 animate-spin"
              style={{ color: "var(--status-info)" }}
            />
            {/* Glow effect */}
            <div
              className="absolute inset-0 rounded-full animate-pulse"
              style={{
                background: "radial-gradient(circle, var(--status-info-border) 0%, transparent 70%)",
              }}
            />
          </div>
        )}
        {status === "active" && isHistorical && (
          <Circle className="w-5 h-5" style={{ color: "var(--status-info)" }} />
        )}
        {status === "pending" && (
          <Circle
            className="w-5 h-5 text-text-primary/20"
          />
        )}
      </div>

      {/* Label */}
      <span
        className="text-[0.8125rem] font-medium"
        style={{
          color:
            status === "completed"
              ? withAlpha("var(--text-primary)", 60)
              : status === "active"
              ? isHistorical
                ? withAlpha("var(--text-primary)", 35)
                : "var(--status-info)"
              : withAlpha("var(--text-primary)", 35),
        }}
      >
        {label}
      </span>
    </div>
  );
}

/**
 * ReviewStepsCard - Shows all review steps with progress
 */
function ReviewStepsCard({
  isHistorical,
  mode,
  variant,
}: {
  isHistorical?: boolean;
  mode: "completed" | "in_progress";
  variant: "success" | "warning" | "error" | "info";
}) {
  const steps: ReviewStep[] =
    mode === "completed"
      ? [
          { label: "Gathering context", status: "completed" },
          { label: "Examining changes", status: "completed" },
          { label: "Running checks", status: "completed" },
          { label: "Generating feedback", status: "completed" },
        ]
      : [
          { label: "Gathering context", status: "completed" },
          { label: "Examining changes", status: "active" },
          { label: "Running checks", status: "pending" },
          { label: "Generating feedback", status: "pending" },
        ];

  return (
    <div data-variant={variant}>
      {steps.map((step, index) => (
        <ReviewStepItem
          key={index}
          {...step}
          isLast={index === steps.length - 1}
          isHistorical={isHistorical === true}
        />
      ))}
    </div>
  );
}

function findOutcomeForTimestamp(
  history: ReviewNoteResponse[],
  timestamp: string | undefined
): ReviewNoteResponse | null {
  if (!timestamp) return null;
  const target = new Date(timestamp).getTime();
  const sorted = [...history].sort(
    (a, b) => new Date(a.created_at).getTime() - new Date(b.created_at).getTime()
  );
  return sorted.find((entry) => new Date(entry.created_at).getTime() >= target) ?? null;
}

function getOutcomeConfig(outcome: ReviewNoteResponse | null) {
  if (!outcome) {
    return {
      title: "AI Review in Progress",
      subtitle: "Outcome not recorded",
      label: "In Progress",
      variant: "info" as const,
      icon: Bot,
      pillIcon: Sparkles,
      mode: "in_progress" as const,
    };
  }

  switch (outcome.outcome) {
    case "approved":
      return {
        title: "AI Review Completed",
        subtitle: "Outcome: Approved",
        label: "Approved",
        variant: "success" as const,
        icon: CheckCircle2,
        pillIcon: CheckCircle2,
        mode: "completed" as const,
      };
    case "changes_requested":
      return {
        title: "AI Review Completed",
        subtitle: "Outcome: Changes Requested",
        label: "Changes Requested",
        variant: "warning" as const,
        icon: AlertTriangle,
        pillIcon: AlertTriangle,
        mode: "completed" as const,
      };
    case "rejected":
      return {
        title: "AI Review Completed",
        subtitle: "Outcome: Rejected",
        label: "Rejected",
        variant: "error" as const,
        icon: XCircle,
        pillIcon: XCircle,
        mode: "completed" as const,
      };
    default:
      return {
        title: "AI Review in Progress",
        subtitle: "Outcome not recorded",
        label: "In Progress",
        variant: "info" as const,
        icon: Bot,
        pillIcon: Sparkles,
        mode: "in_progress" as const,
      };
  }
}

export function ReviewingTaskDetail({
  task,
  isHistorical,
  viewTimestamp,
}: ReviewingTaskDetailProps) {
  const { data: history = [] } = useTaskStateHistory(task.id, {
    enabled: isHistorical === true,
  });
  const outcome = isHistorical ? findOutcomeForTimestamp(history, viewTimestamp) : null;
  const outcomeConfig = isHistorical ? getOutcomeConfig(outcome) : null;

  // Live validation events for setup/install progress
  const liveValidationSteps = useValidationEvents(task.id, "review");

  // Action buttons state
  const queryClient = useQueryClient();
  const { confirm, confirmationDialogProps, ConfirmationDialog } = useConfirmation();
  const [actionError, setActionError] = useState<string | null>(null);
  const [showFeedback, setShowFeedback] = useState(false);
  const [feedback, setFeedback] = useState("");
  const stopGate = useAgentGate("taskStop");
  const requestChangesGate = useAgentGate("taskRequestChangesFromReviewing");

  const stopMutation = useMutation({
    mutationFn: async () => {
      await api.tasks.stop(task.id);
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: taskKeys.all });
      setActionError(null);
    },
    onError: (err) => {
      setActionError(err instanceof Error ? err.message : "Failed to stop review");
    },
  });

  const requestChangesMutation = useMutation({
    mutationFn: (feedbackText: string) =>
      api.reviews.requestTaskChangesFromReviewing({ task_id: task.id, feedback: feedbackText }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: taskKeys.all });
      setShowFeedback(false);
      setFeedback("");
      setActionError(null);
    },
    onError: (err) => {
      setActionError(err instanceof Error ? err.message : "Failed to request changes");
    },
  });

  const handleStop = useCallback(async () => {
    const confirmed = await confirm({
      title: "Stop review?",
      description: "This will stop the AI review and move the task back to a stoppable state.",
      confirmText: "Stop Review",
      variant: "destructive",
    });
    if (!confirmed) return;
    stopMutation.mutate();
  }, [confirm, stopMutation]);

  const handleRequestChanges = () => {
    if (showFeedback && feedback.trim().length > 0) {
      requestChangesMutation.mutate(feedback.trim());
    } else if (showFeedback && feedback.trim().length === 0) {
      setActionError("Feedback cannot be empty");
    } else {
      setShowFeedback(true);
      setActionError(null);
    }
  };

  const isActionLoading = stopMutation.isPending || requestChangesMutation.isPending;

  return (
    <>
      <TwoColumnLayout
        description={task.description}
        testId="reviewing-task-detail"
        actions={
          !isHistorical ? (
            <section data-testid="reviewing-actions-section">
              <SectionTitle>Actions</SectionTitle>
              <DetailCard>
                {showFeedback && (
                  <div className="mb-4 space-y-3">
                    <Textarea
                      data-testid="feedback-input"
                      value={feedback}
                      onChange={(e) => setFeedback(e.target.value)}
                      placeholder="Describe the changes needed..."
                      disabled={requestChangesMutation.isPending}
                      className="min-h-[100px] text-[0.8125rem] resize-none rounded-xl"
                      style={{
                        backgroundColor: "var(--overlay-scrim)",
                        borderColor: "var(--overlay-moderate)",
                        borderStyle: "solid",
                        borderWidth: "1px",
                      }}
                    />
                  </div>
                )}
                <div className="flex items-center gap-2">
                  <AgentGateTooltip gated={stopGate.gated} reason={stopGate.reason}><button
                    type="button"
                    data-testid="stop-review-action"
                    onClick={handleStop}
                    disabled={isActionLoading || stopGate.gated}
                    className="flex items-center gap-1.5 px-3 py-1.5 rounded-md text-[0.75rem] font-medium transition-colors cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed"
                    style={{
                      backgroundColor: statusTint("error", 15),
                      color: "var(--status-error)",
                    }}
                  >
                    <Square className="w-3.5 h-3.5" />
                    Stop Review
                  </button></AgentGateTooltip>
                  <AgentGateTooltip gated={requestChangesGate.gated} reason={requestChangesGate.reason}><button
                    type="button"
                    data-testid="request-changes-action"
                    onClick={handleRequestChanges}
                    disabled={requestChangesMutation.isPending || (showFeedback && feedback.trim().length === 0) || requestChangesGate.gated}
                    className="flex items-center gap-1.5 px-3 py-1.5 rounded-md text-[0.75rem] font-medium transition-colors cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed"
                    style={{
                      backgroundColor: "var(--status-warning-muted)",
                      color: "var(--status-warning)",
                    }}
                  >
                    {requestChangesMutation.isPending ? (
                      <Loader2 className="w-3.5 h-3.5 animate-spin" />
                    ) : (
                      <RotateCcw className="w-3.5 h-3.5" />
                    )}
                    {requestChangesMutation.isPending
                      ? "Submitting..."
                      : showFeedback
                      ? "Submit"
                      : "Request Changes"}
                  </button></AgentGateTooltip>
                  {showFeedback && !requestChangesMutation.isPending && (
                    <button
                      type="button"
                      data-testid="cancel-request-changes"
                      onClick={() => {
                        setShowFeedback(false);
                        setFeedback("");
                        setActionError(null);
                      }}
                      className="text-[0.75rem] text-text-primary/40 hover:text-text-primary/60 transition-colors cursor-pointer"
                    >
                      Cancel
                    </button>
                  )}
                </div>
                {actionError && (
                  <p className="mt-1 text-[0.75rem]" style={{ color: "var(--status-error)" }}>
                    {actionError}
                  </p>
                )}
              </DetailCard>
            </section>
          ) : null
        }
      >
        {/* Status Banner */}
        <StatusBanner
          icon={isHistorical ? outcomeConfig?.icon ?? Bot : Bot}
          title={isHistorical ? outcomeConfig?.title ?? "AI Review in Progress" : "AI Review in Progress"}
          subtitle={
            isHistorical
              ? outcomeConfig?.subtitle ?? "Analyzing changes and running checks"
              : "Analyzing changes and running checks"
          }
          variant={isHistorical ? outcomeConfig?.variant ?? "info" : "info"}
          animated={!isHistorical}
          badge={
            <StatusPill
              icon={isHistorical ? outcomeConfig?.pillIcon ?? Sparkles : Sparkles}
              label={isHistorical ? outcomeConfig?.label ?? "In Progress" : "Analyzing"}
              variant={isHistorical ? outcomeConfig?.variant ?? "info" : "info"}
              animated={!isHistorical}
              size="md"
            />
          }
        />

        {/* Duration — live while reviewing, static in historical mode */}
        {task.startedAt && (
          <div data-testid="reviewing-task-duration">
            <DurationDisplay
              mode={isHistorical ? "static" : "live"}
              startedAt={task.startedAt}
              completedAt={isHistorical ? task.completedAt : null}
            />
          </div>
        )}

        {/* Setup/Install Progress (live validation events) */}
        <ValidationProgress
          taskId={task.id}
          metadata={task.metadata}
          liveSteps={liveValidationSteps}
          title="Environment Setup"
          metadataLogKey="review_setup_log"
        />

        <TaskValidationSection
          taskId={task.id}
          isHistorical={isHistorical === true}
        />

        {/* Review Steps */}
        <section data-testid="reviewing-steps-section">
          <SectionTitle>Review Progress</SectionTitle>
          <ReviewStepsCard
            isHistorical={isHistorical === true}
            mode={isHistorical ? outcomeConfig?.mode ?? "in_progress" : "in_progress"}
            variant={isHistorical ? outcomeConfig?.variant ?? "info" : "info"}
          />
        </section>

      </TwoColumnLayout>
      {!isHistorical && <ConfirmationDialog {...confirmationDialogProps} />}
    </>
  );
}
