/**
 * BasicTaskDetail - macOS Tahoe-inspired basic task view
 *
 * Clean, spacious layout for simple task states (backlog, ready, blocked).
 * Features native vibrancy materials and refined typography.
 */

import { useCallback, useMemo, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { formatDistanceToNow, parseISO } from "date-fns";
import { StepList } from "../StepList";
import { SectionTitle, TwoColumnLayout, DetailCard, StatusBanner } from "./shared";
import { DurationDisplay } from "./shared/DurationDisplay";
import { useTaskSteps } from "@/hooks/useTaskSteps";
import { useConfirmation } from "@/hooks/useConfirmation";
import { taskKeys } from "@/hooks/useTasks";
import { executionKeys } from "@/hooks/useExecutionControl";
import { api } from "@/lib/tauri";
import { Loader2, Play, RotateCcw, Clock, AlertTriangle, ShieldAlert, X, GitBranch } from "lucide-react";
import { useAgentGate } from "@/hooks/useAgentGate";
import { AgentGateTooltip } from "@/components/remote/AgentGateTooltip";
import { Button } from "@/components/ui/button";
import {
  ResumeValidationDialog,
  type ValidationWarning,
} from "@/components/ui/ResumeValidationDialog";
import {
  parseStopMetadata,
  type Task,
  type StopMetadata,
  type FailureSource,
  TRANSIENT_FAILURE_SOURCES,
} from "@/types/task";
import type { InternalStatus } from "@/types/status";
import { stopExecutionRetry } from "@/lib/task-actions/task-actions";
import {
  FRESHNESS_BLOCKED_PREFIX,
  parseFreshnessBlockedReason,
  type FreshnessBlockedInfo,
} from "@/lib/freshness-blocked";
import { resumeExecutionIfStopped } from "@/lib/task-actions/resume-execution-if-stopped";

// ============================================================================
// Helper Functions
// ============================================================================

/**
 * Convert snake_case status to Title Case.
 * "merging" → "Merging"
 * "qa_testing" → "QA Testing"
 * "merge_conflict" → "Merge Conflict"
 */
function formatStatusLabel(status: string): string {
  return status
    .split("_")
    .map((word) => {
      // Handle common abbreviations
      if (word.toUpperCase() === word) return word;
      if (word === "qa") return "QA";
      return word.charAt(0).toUpperCase() + word.slice(1);
    })
    .join(" ");
}

/**
 * Get relative time string from ISO timestamp.
 * Returns "just now" for invalid dates.
 */
function getTimeAgo(isoString: string): string {
  try {
    const date = parseISO(isoString);
    return formatDistanceToNow(date, { addSuffix: true });
  } catch {
    return "just now";
  }
}

// Task statuses that can be restarted
const RESTARTABLE_STATUSES = new Set(["failed", "stopped", "cancelled", "paused"]);
const SUCCESSFUL_TERMINAL_STATUSES = new Set<InternalStatus>(["approved", "merged"]);

// Human-readable labels for failure source codes
const FAILURE_SOURCE_LABELS: Record<string, string> = {
  transient_timeout: "transient timeout",
  agent_crash: "agent crash",
  local_tool_failed: "local tool failure",
  parse_stall: "parse stall",
  git_isolation: "git isolation",
  wall_clock_timeout: "wall-clock timeout",
  max_retries_exceeded: "retry limit reached",
  provider_error: "provider error",
  validation_failed: "validation failed",
};

// States that need validation before resuming (merge-related states)
const VALIDATED_RESUME_STATES = new Set([
  "merging",
  "pending_merge",
  "merge_conflict",
  "merge_incomplete",
]);

interface BasicTaskDetailProps {
  task: Task;
  isHistorical?: boolean;
}

/**
 * StopHistorySection - Shows stop history for stopped tasks
 * Displays original state, stop reason, and time since stopped.
 */
function StopHistorySection({ stopMetadata }: { stopMetadata: StopMetadata }) {
  const fromStatusLabel = formatStatusLabel(stopMetadata.stoppedFromStatus);
  const timeAgo = getTimeAgo(stopMetadata.stoppedAt);

  return (
    <section data-testid="stop-history-section" className="space-y-2">
      <SectionTitle>Stop History</SectionTitle>
      <DetailCard>
        <div className="space-y-3">
          {/* Stopped From Status */}
          <div className="flex items-center gap-2">
            <span
              className="text-[0.6875rem] font-medium uppercase tracking-wider"
              style={{ color: "var(--text-muted)" }}
            >
              Stopped from
            </span>
            <span
              className="text-[0.8125rem] font-medium px-2 py-0.5 rounded"
              style={{
                backgroundColor: "var(--status-warning-muted)",
                color: "var(--status-warning)",
              }}
            >
              {fromStatusLabel}
            </span>
          </div>

          {/* Stop Reason (if provided) */}
          {stopMetadata.stopReason && (
            <div className="mt-2">
              <span
                className="text-[0.6875rem] font-medium uppercase tracking-wider block mb-1"
                style={{ color: "var(--text-muted)" }}
              >
                Reason
              </span>
              <p className="text-[0.8125rem]" style={{ color: "var(--text-secondary)" }}>
                {stopMetadata.stopReason}
              </p>
            </div>
          )}

          {/* Time Ago */}
          <div className="flex items-center gap-2 mt-2">
            <Clock className="w-3.5 h-3.5" style={{ color: "var(--text-muted)" }} />
            <span className="text-[0.75rem]" style={{ color: "var(--text-secondary)" }}>
              {timeAgo}
            </span>
          </div>
        </div>
      </DetailCard>
    </section>
  );
}

/**
 * AutoRetryingSection - Shown for failed tasks that are being auto-retried.
 * Displays attempt count and provides a "Stop Retrying" button.
 */
function AutoRetryingSection({ task, attemptCount }: { task: Task; attemptCount: number }) {
  const queryClient = useQueryClient();

  const stopRetryMutation = useMutation({
    mutationFn: () => stopExecutionRetry(task.id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: taskKeys.all });
    },
  });

  return (
    <section data-testid="auto-retrying-section" className="space-y-2">
      <SectionTitle>Execution Recovery</SectionTitle>
      <DetailCard>
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <Loader2 className="w-4 h-4 animate-spin" style={{ color: "var(--accent-primary)" }} />
            <span
              data-testid="auto-retry-badge"
              className="text-[0.8125rem] font-medium"
              style={{ color: "var(--accent-primary)" }}
            >
              Auto-retrying{attemptCount > 0 ? ` (attempt ${attemptCount})` : ""}
            </span>
          </div>
          <Button
            data-testid="stop-retrying-button"
            onClick={() => stopRetryMutation.mutate()}
            disabled={stopRetryMutation.isPending}
            className="h-8 px-3 gap-1.5 rounded-lg font-medium text-[0.75rem] transition-colors"
            style={{
              backgroundColor: "var(--status-error-muted)",
              color: "var(--status-error)",
            }}
          >
            {stopRetryMutation.isPending ? (
              <Loader2 className="w-3.5 h-3.5 animate-spin" />
            ) : (
              <X className="w-3.5 h-3.5" />
            )}
            Stop Retrying
          </Button>
        </div>
        {stopRetryMutation.error && (
          <p className="mt-2 text-[0.75rem]" style={{ color: "var(--status-error)" }}>
            {stopRetryMutation.error.message}
          </p>
        )}
      </DetailCard>
    </section>
  );
}

/**
 * ActionButtonsCard - Restart button for terminal/suspended states
 * For stopped tasks with stop metadata, shows enhanced confirmation dialog.
 */
function ActionButtonsCard({ task }: { task: Task }) {
  const queryClient = useQueryClient();
  const { confirm, confirmationDialogProps, ConfirmationDialog } = useConfirmation();
  const [showValidationDialog, setShowValidationDialog] = useState(false);
  const [isResuming, setIsResuming] = useState(false);
  const [restartNote, setRestartNote] = useState("");
  const [restartOutcome, setRestartOutcome] = useState<string | null>(null);
  const taskId = task.id;
  const status = task.internalStatus;

  // Parse stop metadata for enhanced confirmation dialog
  const stopMetadata = useMemo(
    () => parseStopMetadata(task.metadata),
    [task.metadata]
  );

  const isStopped = status === "stopped" && stopMetadata !== null;
  const isFailed = status === "failed";
  const isReady = status === "ready";

  // Generate validation warnings based on stopped-from state
  const validationWarnings = useMemo((): ValidationWarning[] => {
    if (!stopMetadata) return [];

    const warnings: ValidationWarning[] = [];
    const stoppedFrom = stopMetadata.stoppedFromStatus;

    // Check if this was a merge-related state
    if (VALIDATED_RESUME_STATES.has(stoppedFrom)) {
      warnings.push({
        id: "git-state",
        message: `Task was stopped during ${stoppedFrom.replace("_", " ")} phase. Git state may have changed since then.`,
        severity: "warning",
      });

      warnings.push({
        id: "branch-check",
        message: "The task branch and worktree should be verified before resuming.",
        severity: "warning",
      });
    }

    // Add stop reason as context if available
    if (stopMetadata.stopReason) {
      warnings.push({
        id: "stop-reason",
        message: `Original stop reason: "${stopMetadata.stopReason}"`,
        severity: "warning",
      });
    }

    return warnings;
  }, [stopMetadata]);

  const restartMutation = useMutation({
    mutationFn: async () => {
      const note = restartNote.trim() || undefined;
      if (isStopped || isFailed) {
        // Backend owns smart resume and failed-task recover-or-restart classification.
        const result = await api.tasks.restart(taskId, false, note);
        if (result.type === "ValidationFailed") {
          throw new Error(
            `Validation failed: ${result.warnings.map((w) => w.message).join(", ")}`
          );
        }
        if (result.type === "Blocked") {
          throw new Error(result.warnings.map((warning) => warning.message).join(", "));
        }
        await resumeExecutionIfStopped(task.projectId);
        return result;
      } else {
        // Fallback to move-to-ready for other restartable statuses
        const result = await api.tasks.move(taskId, "ready", note);
        await resumeExecutionIfStopped(task.projectId);
        return result;
      }
    },
    onMutate: () => {
      setRestartOutcome(null);
    },
    onSuccess: (result) => {
      setRestartNote("");
      if ("disposition" in result) {
        setRestartOutcome(
          result.disposition === "recovered_to_review"
            ? "Completed work recovered and continued to review."
            : result.disposition === "restarted_to_ready"
              ? "A fresh execution attempt was started."
              : null
        );
      }
    },
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: taskKeys.all });
      queryClient.invalidateQueries({ queryKey: executionKeys.all });
    },
  });

  // Handle force resume from validation dialog - restores to original state (smart resume)
  const handleForceResume = useCallback(async () => {
    if (!stopMetadata?.stoppedFromStatus) return;
    setIsResuming(true);
    try {
      const note = restartNote.trim() || undefined;
      await api.tasks.move(taskId, stopMetadata.stoppedFromStatus, note);
      await resumeExecutionIfStopped(task.projectId);
      setRestartNote("");
      setShowValidationDialog(false);
    } finally {
      queryClient.invalidateQueries({ queryKey: taskKeys.all });
      queryClient.invalidateQueries({ queryKey: executionKeys.all });
      setIsResuming(false);
    }
  }, [queryClient, restartNote, stopMetadata, task.projectId, taskId]);

  // Handle go to ready from validation dialog
  const handleGoToReady = useCallback(async () => {
    setIsResuming(true);
    try {
      const note = restartNote.trim() || undefined;
      await api.tasks.move(taskId, "ready", note);
      await resumeExecutionIfStopped(task.projectId);
      setRestartNote("");
      setShowValidationDialog(false);
    } finally {
      queryClient.invalidateQueries({ queryKey: taskKeys.all });
      queryClient.invalidateQueries({ queryKey: executionKeys.all });
      setIsResuming(false);
    }
  }, [queryClient, restartNote, task.projectId, taskId]);

  const handleAction = useCallback(async () => {
    // If task was stopped from a validated state, show validation dialog
    if (stopMetadata && validationWarnings.length > 0) {
      setShowValidationDialog(true);
      return;
    }

    const statusLabels: Record<string, string> = {
      ready: "Start",
      failed: "Restart",
      stopped: "Restart",
      cancelled: "Restart",
      paused: "Resume",
    };
    const actionLabel = statusLabels[status] || "Restart";
    const taskLabel = isReady
      ? "task"
      : status === "paused"
        ? "paused task"
        : "failed task";

    // Build enhanced confirmation for stopped tasks with metadata
    if (isStopped && stopMetadata) {
      const fromStatusLabel = formatStatusLabel(stopMetadata.stoppedFromStatus);
      const timeAgo = getTimeAgo(stopMetadata.stoppedAt);

      const descriptionParts = [
        `Original state: ${fromStatusLabel}`,
        stopMetadata.stopReason && `Reason: ${stopMetadata.stopReason}`,
        `Stopped ${timeAgo}`,
        "",
        "The task will resume with smart state restoration.",
      ];
      const description = descriptionParts.filter(Boolean).join("\n");

      const confirmed = await confirm({
        title: `Restart this stopped task?`,
        description,
        confirmText: actionLabel,
        variant: "default",
      });
      if (!confirmed) return;
    } else {
      const confirmed = await confirm({
        title: `${actionLabel} this ${taskLabel}?`,
        description: isReady
          ? "The task will be started."
          : isFailed
            ? "RalphX will recover the existing completed work and continue to review when current-attempt proof is valid. Otherwise it will safely start a fresh execution attempt. Existing work is not cleared if the safety checks cannot complete."
            : "The task will be moved to ready status and can be executed again.",
        confirmText: actionLabel,
        variant: "default",
      });
      if (!confirmed) return;
    }

    restartMutation.mutate();
  }, [confirm, status, isReady, isStopped, isFailed, stopMetadata, restartMutation, validationWarnings.length]);

  return (
    <DetailCard data-testid="action-buttons">
      <div className="flex items-center justify-between">
        <span
          className="text-[0.6875rem] font-semibold uppercase tracking-wider"
          style={{ color: "var(--text-muted)" }}
        >
          Actions
        </span>
        <Button
          data-testid={isReady ? "start-button" : "restart-button"}
          onClick={handleAction}
          disabled={restartMutation.isPending || isResuming}
          className="h-9 px-4 gap-2 rounded-lg font-medium text-[0.8125rem] transition-colors"
          style={{
            backgroundColor: isReady ? "var(--accent-primary)" : "var(--status-info)",
            color: "white",
          }}
        >
          {restartMutation.isPending || isResuming ? (
            <Loader2 className="w-4 h-4 animate-spin" />
          ) : isReady ? (
            <Play className="w-4 h-4" />
          ) : (
            <RotateCcw className="w-4 h-4" />
          )}
          {isReady ? "Start" : status === "paused" ? "Resume" : "Restart"}
        </Button>
      </div>

      {/* Restart Note textarea (for restartable states only, not shown for start) */}
      {!isReady && (
        <div className="mt-3 pt-3" style={{ borderTop: "1px solid var(--overlay-weak)" }}>
        <textarea
          aria-label="Restart note"
          data-testid="restart-note-textarea"
            value={restartNote}
            onChange={(e) => setRestartNote(e.target.value)}
            disabled={restartMutation.isPending || isResuming}
            placeholder="Optional: tell the agent what to do differently..."
            rows={3}
            className="w-full resize-none rounded-md px-3 py-2 text-[0.75rem] transition-colors disabled:opacity-40 outline-none ring-0 focus:ring-0 focus:outline-none focus-visible:outline-none border-0"
            style={{
              backgroundColor: "var(--overlay-weak)",
              color: "var(--text-secondary)",
              boxShadow: "none",
              outline: "none",
            }}
          />
        </div>
      )}

      {/* Error display */}
      {restartMutation.error && (
        <p className="mt-3 text-[0.75rem]" style={{ color: "var(--status-error)" }}>
          {restartMutation.error.message}
        </p>
      )}
      {restartOutcome && (
        <p data-testid="restart-outcome" className="mt-3 text-[0.75rem]" style={{ color: "var(--status-success)" }}>
          {restartOutcome}
        </p>
      )}

      <ConfirmationDialog {...confirmationDialogProps} />

      {/* Resume Validation Dialog */}
      <ResumeValidationDialog
        isOpen={showValidationDialog}
        onClose={() => setShowValidationDialog(false)}
        onForceResume={handleForceResume}
        onGoToReady={handleGoToReady}
        taskTitle={task.title}
        stoppedFromStatus={stopMetadata?.stoppedFromStatus}
        warnings={validationWarnings}
        isLoading={isResuming}
      />
    </DetailCard>
  );
}

/**
 * UnblockWarningCard - Shown for blocked tasks with failed dependencies.
 * Presents an unblock action with a confirmation dialog warning that
 * the upstream dependency failed.
 */
function UnblockWarningCard({
  task,
  failedDependencyName,
}: {
  task: Task;
  failedDependencyName: string;
}) {
  const queryClient = useQueryClient();
  const { confirm, confirmationDialogProps, ConfirmationDialog } = useConfirmation();

  const unblockMutation = useMutation({
    mutationFn: () => api.tasks.unblock(task.id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: taskKeys.all });
    },
  });

  const agentGate = useAgentGate("taskUnblock");

  const handleUnblock = useCallback(async () => {
    const confirmed = await confirm({
      title: "Unblock despite failed dependency?",
      description: `"${failedDependencyName}" failed. Unblocking will move this task to Ready, but it may run against incomplete output. Proceed only if you understand the risk.`,
      confirmText: "Unblock Anyway",
      variant: "default",
    });
    if (!confirmed) return;
    unblockMutation.mutate();
  }, [confirm, failedDependencyName, unblockMutation]);

  return (
    <DetailCard>
      <div data-testid="unblock-warning-card" className="flex items-center justify-between">
        <span
          className="text-[0.6875rem] font-semibold uppercase tracking-wider"
          style={{ color: "var(--text-muted)" }}
        >
          Actions
        </span>
        <AgentGateTooltip
          gated={agentGate.gated}
          reason={agentGate.reason}
          testId="unblock-button-gate"
        >
        <Button
          data-testid="unblock-button"
          onClick={handleUnblock}
          disabled={unblockMutation.isPending || agentGate.gated}
          className="h-9 px-4 gap-2 rounded-lg font-medium text-[0.8125rem]"
          style={{
            backgroundColor: "var(--status-warning)",
            color: "white",
          }}
        >
          {unblockMutation.isPending ? (
            <Loader2 className="w-4 h-4 animate-spin" />
          ) : (
            <ShieldAlert className="w-4 h-4" />
          )}
          Unblock Anyway
        </Button>
        </AgentGateTooltip>
      </div>
      {unblockMutation.error && (
        <p className="mt-3 text-[0.75rem]" style={{ color: "var(--status-error)" }}>
          {unblockMutation.error.message}
        </p>
      )}
      <ConfirmationDialog {...confirmationDialogProps} />
    </DetailCard>
  );
}

/**
 * FreshnessBlockedCard - Shown when a task is blocked due to persistent branch freshness conflicts.
 * Displays conflict files, attempt/elapsed info, and a one-click "Reset & Retry" recovery action.
 */
function FreshnessBlockedCard({
  task,
  info,
}: {
  task: Task;
  info: FreshnessBlockedInfo;
}) {
  const queryClient = useQueryClient();

  const resetMutation = useMutation({
    mutationFn: async () => {
      const result = await api.tasks.move(task.id, "ready");
      await resumeExecutionIfStopped(task.projectId);
      return result;
    },
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: taskKeys.all });
      queryClient.invalidateQueries({ queryKey: executionKeys.all });
    },
  });

  const elapsedLabel =
    info.elapsedMinutes > 0 ? `${info.elapsedMinutes} min` : "unknown duration";

  return (
    <section data-testid="freshness-blocked-section" className="space-y-2">
      <SectionTitle>Branch Freshness Blocked</SectionTitle>

      {/* Summary banner */}
      <div
        className="rounded-xl p-4 space-y-3"
        style={{ backgroundColor: "var(--status-warning-muted)" }}
      >
        <div className="flex items-start gap-2.5">
          <GitBranch
            className="w-4 h-4 mt-0.5 shrink-0"
            style={{ color: "var(--status-warning)" }}
          />
          <div className="flex-1 min-w-0">
            <p
              data-testid="freshness-blocked-summary"
              className="text-[0.8125rem] font-medium"
              style={{ color: "var(--status-warning)" }}
            >
              Branch conflicts could not be resolved automatically after{" "}
              {info.totalAttempts} {info.totalAttempts === 1 ? "attempt" : "attempts"} over{" "}
              {elapsedLabel}.
            </p>

            {/* Conflict file list */}
            {info.conflictFiles.length > 0 && (
              <div className="mt-2.5 space-y-1">
                <span
                  className="text-[0.6875rem] font-semibold uppercase tracking-wider block"
                  style={{ color: "var(--status-warning)" }}
                >
                  Conflict files
                </span>
                <ul className="space-y-0.5">
                  {info.conflictFiles.map((file) => (
                    <li
                      key={file}
                      data-testid="freshness-conflict-file"
                      className="text-[0.75rem] font-mono truncate"
                      style={{ color: "color-mix(in srgb, var(--status-warning) 85%, transparent)" }}
                    >
                      {file}
                    </li>
                  ))}
                </ul>
              </div>
            )}
          </div>
        </div>
      </div>

      {/* Actions */}
      <DetailCard>
        <div className="flex items-center justify-between gap-3">
          {/* Secondary: task branch label */}
          {task.taskBranch && (
            <div className="flex items-center gap-1.5 min-w-0 overflow-hidden">
              <GitBranch className="w-3 h-3 shrink-0" style={{ color: "var(--text-muted)" }} />
              <span
                className="text-[0.6875rem] font-mono truncate"
                style={{ color: "var(--text-muted)" }}
                title={task.taskBranch}
              >
                {task.taskBranch}
              </span>
            </div>
          )}

          {/* Primary: Reset & Retry */}
          <Button
            data-testid="freshness-reset-retry-button"
            onClick={() => resetMutation.mutate()}
            disabled={resetMutation.isPending}
            className="h-9 px-4 gap-2 rounded-lg font-medium text-[0.8125rem] shrink-0 transition-colors"
            style={{
              backgroundColor: "var(--status-warning)",
              color: "var(--bg-surface)",
            }}
          >
            {resetMutation.isPending ? (
              <Loader2 className="w-4 h-4 animate-spin" />
            ) : (
              <RotateCcw className="w-4 h-4" />
            )}
            Reset & Retry
          </Button>
        </div>

        {resetMutation.error && (
          <p className="mt-3 text-[0.75rem]" style={{ color: "var(--status-error)" }}>
            {resetMutation.error.message}
          </p>
        )}
      </DetailCard>
    </section>
  );
}

export function BasicTaskDetail({ task, isHistorical = false }: BasicTaskDetailProps) {
  const { data: steps, isLoading: stepsLoading } = useTaskSteps(task.id);
  const hasSteps = (steps?.length ?? 0) > 0;
  const isRestartable = RESTARTABLE_STATUSES.has(task.internalStatus);
  const showsActions = isRestartable || task.internalStatus === "ready";

  // Parse stop metadata for stopped tasks
  const stopMetadata = useMemo(
    () => parseStopMetadata(task.metadata),
    [task.metadata]
  );
  const isStopped = task.internalStatus === "stopped" && stopMetadata !== null;

  // Parse failure info from task metadata when task is failed or qa_failed
  let failureInfo: {
    failure_error: string;
    failure_details?: string;
    is_timeout: boolean;
    attempt_count: number;
    failure_source?: FailureSource;
    failed_at?: string;
  } | null = null;

  const isFailed = task.internalStatus === "failed" || task.internalStatus === "qa_failed";

  if (isFailed) {
    if (task.metadata) {
      try {
        const metadata = JSON.parse(task.metadata);
        const failureError =
          typeof metadata.failure_error === "string" ? metadata.failure_error : "";
        const lastAgentError =
          typeof metadata.last_agent_error === "string" ? metadata.last_agent_error : "";
        const displayError = failureError || lastAgentError;
        if (displayError) {
          const attemptCount =
            typeof metadata.auto_retry_count_executing === "number"
              ? metadata.auto_retry_count_executing
              : 0;
          failureInfo = {
            failure_error: displayError,
            is_timeout: metadata.is_timeout || false,
            attempt_count: attemptCount,
            ...(metadata.failure_details !== undefined && { failure_details: metadata.failure_details as string }),
            ...(typeof metadata.failure_source === "string" && { failure_source: metadata.failure_source as FailureSource }),
            ...(typeof metadata.failed_at === "string" && { failed_at: metadata.failed_at }),
          };
        }
      } catch {
        // JSON parse failed - fall through to fallback
      }
    }

    // Fallback handling for null/invalid metadata
    if (!failureInfo) {
      // Try blockedReason first (set by ExecutionBlocked handler)
      const errorMessage = task.blockedReason ||
        "Task execution failed. Error details were not recorded during the state transition.";
      failureInfo = {
        failure_error: errorMessage,
        is_timeout: false,
        attempt_count: 0,
      };
    }
  }

  // Parse execution_recovery metadata for failed tasks (Wave 4 auto-retry UI)
  // Three sub-states: auto-retrying (last_state=retrying), permanently failed, legacy (no metadata)
  let executionRecovery: {
    last_state: "retrying" | "failed" | "succeeded";
    stop_retrying: boolean;
    attempt_count: number;
  } | null = null;

  if (task.internalStatus === "failed") {
    if (task.metadata) {
      try {
        const metadata = JSON.parse(task.metadata);
        if (metadata.execution_recovery) {
          const er = metadata.execution_recovery as {
            last_state?: string;
            stop_retrying?: boolean;
            events?: Array<{ kind: string }>;
          };
          const events = er.events ?? [];
          const autoRetryCount = events.filter((e) => e.kind === "auto_retry_triggered").length;
          executionRecovery = {
            last_state: (er.last_state ?? "failed") as "retrying" | "failed" | "succeeded",
            stop_retrying: er.stop_retrying ?? false,
            attempt_count: autoRetryCount,
          };
        }
      } catch {
        // JSON parse failed — treat as no metadata
      }
    }
  }

  // Auto-retrying: system is managing retries, show badge + Stop Retrying button
  const isAutoRetrying =
    executionRecovery?.last_state === "retrying" && !executionRecovery.stop_retrying;

  // Detect if a blocked task has a failed dependency as its blocker.
  // The backend sets blocked_reason to "Dependency [task title] failed." when a blocker fails.
  const DEPENDENCY_FAILED_PREFIX = "Dependency ";
  const DEPENDENCY_FAILED_SUFFIX = " failed.";
  const isBlockedByFailedDependency =
    task.internalStatus === "blocked" &&
    task.blockedReason !== null &&
    task.blockedReason !== undefined &&
    task.blockedReason.startsWith(DEPENDENCY_FAILED_PREFIX) &&
    task.blockedReason.endsWith(DEPENDENCY_FAILED_SUFFIX);
  const failedDependencyName = isBlockedByFailedDependency
    ? task.blockedReason!.slice(
        DEPENDENCY_FAILED_PREFIX.length,
        task.blockedReason!.length - DEPENDENCY_FAILED_SUFFIX.length
      )
    : null;

  // Detect freshness-blocked tasks: blocked status with FRESHNESS_BLOCKED| structured reason
  const isFreshnessBlocked =
    task.internalStatus === "blocked" &&
    task.blockedReason !== null &&
    task.blockedReason !== undefined &&
    task.blockedReason.startsWith(FRESHNESS_BLOCKED_PREFIX);
  const freshnessBlockedInfo = isFreshnessBlocked
    ? parseFreshnessBlockedReason(task.blockedReason!)
    : null;

  // Parse current agent errors, but do not surface stale failures after successful terminal states.
  const shouldShowAgentError = !SUCCESSFUL_TERMINAL_STATUSES.has(task.internalStatus);
  const agentError = useMemo(() => {
    if (!shouldShowAgentError) return null;
    if (!task.metadata) return null;
    try {
      const metadata = JSON.parse(task.metadata);
      const lastError = metadata.last_agent_error;
      if (!lastError) return null;
      const errorContext: string | undefined = metadata.last_agent_error_context;
      const contextLabel =
        errorContext === "review" ? "Reviewer"
        : errorContext === "execution" ? "Worker"
        : "Agent";
      return {
        message: lastError as string,
        contextLabel,
        errorAt: metadata.last_agent_error_at as string | undefined,
      };
    } catch {
      return null;
    }
  }, [shouldShowAgentError, task.metadata]);

  return (
    <TwoColumnLayout
      description={task.description}
      testId="basic-task-detail"
    >
      {/* Stop History Section (for stopped tasks with metadata) */}
      {isStopped && stopMetadata && (
        <StopHistorySection stopMetadata={stopMetadata} />
      )}

      {/* Dependency Failed Warning Banner (for blocked tasks with failed dependency) */}
      {isBlockedByFailedDependency && failedDependencyName && (
        <section data-testid="dependency-failed-banner">
          <StatusBanner
            icon={ShieldAlert}
            title="Dependency Failed"
            subtitle={`"${failedDependencyName}" failed — this task cannot proceed until manually unblocked.`}
            variant="warning"
          />
        </section>
      )}

      {/* Freshness Blocked recovery UI */}
      {!isHistorical && isFreshnessBlocked && freshnessBlockedInfo && (
        <FreshnessBlockedCard task={task} info={freshnessBlockedInfo} />
      )}

      {/* Auto-Retrying Banner (shown when system is managing retries) */}
      {isAutoRetrying && (
        <AutoRetryingSection
          task={task}
          attemptCount={executionRecovery?.attempt_count ?? 0}
        />
      )}

      {/* Failure Reason Banner */}
      {failureInfo && (
        <section data-testid="failure-reason-section" className="space-y-2">
          <SectionTitle>Failure Details</SectionTitle>
          <div
            className="rounded-xl p-4 space-y-3"
            style={{ backgroundColor: "var(--status-error-muted)" }}
          >
            {/* Attempt count */}
            {failureInfo.attempt_count > 0 && (
              <div data-testid="attempt-count" className="flex items-center gap-2">
                <span
                  className="text-[0.6875rem] font-semibold uppercase tracking-wider"
                  style={{ color: "var(--status-error)" }}
                >
                  Failed after {failureInfo.attempt_count}{" "}
                  {failureInfo.attempt_count === 1 ? "attempt" : "attempts"}
                </span>
                {failureInfo.is_timeout && (
                  <span
                    data-testid="timeout-badge"
                    className="text-[0.625rem] font-semibold px-2 py-0.5 rounded-full"
                    style={{
                      backgroundColor: "var(--status-error-border)",
                      color: "var(--status-error)",
                    }}
                  >
                    timeout
                  </span>
                )}
              </div>
            )}
            {/* Show timeout badge even when attempt_count is 0 */}
            {failureInfo.attempt_count === 0 && failureInfo.is_timeout && (
              <div className="flex items-center gap-2">
                <span
                  data-testid="timeout-badge"
                  className="text-[0.625rem] font-semibold px-2 py-0.5 rounded-full"
                  style={{
                    backgroundColor: "var(--status-error-border)",
                    color: "var(--status-error)",
                  }}
                >
                  timeout
                </span>
              </div>
            )}
            {/* Failure source badge — amber for transient (auto-retrying), red for terminal */}
            {failureInfo.failure_source && failureInfo.failure_source !== "unknown" && (() => {
              const isTerminal =
                executionRecovery?.stop_retrying === true ||
                !TRANSIENT_FAILURE_SOURCES.has(failureInfo.failure_source!);
              const label = FAILURE_SOURCE_LABELS[failureInfo.failure_source!] ?? failureInfo.failure_source;
              return (
                <div data-testid="failure-source-badge" className="flex items-center gap-2">
                  <span
                    className="text-[0.625rem] font-semibold px-2 py-0.5 rounded-full"
                    style={
                      isTerminal
                        ? { backgroundColor: "var(--status-error-border)", color: "var(--status-error)" }
                        : { backgroundColor: "var(--status-warning-muted)", color: "var(--status-warning)" }
                    }
                  >
                    {isTerminal ? "Manual retry required" : "Auto-retrying"}
                  </span>
                  <span
                    className="text-[0.625rem]"
                    style={{ color: "color-mix(in srgb, var(--status-error) 65%, transparent)" }}
                  >
                    {label}
                  </span>
                </div>
              );
            })()}
            {/* Error message */}
            <p
              data-testid="failure-error-message"
              className="text-[0.8125rem]"
              style={{ color: "var(--status-error)" }}
            >
              {failureInfo.failure_error}
            </p>
            {/* Failed at relative timestamp */}
            {failureInfo.failed_at && (
              <p
                data-testid="failed-at-timestamp"
                className="text-[0.6875rem]"
                style={{ color: "color-mix(in srgb, var(--status-error) 60%, transparent)" }}
              >
                Failed {getTimeAgo(failureInfo.failed_at)}
              </p>
            )}
            {/* Details */}
            {failureInfo.failure_details && (
              <p
                data-testid="failure-details"
                className="text-[0.75rem]"
                style={{ color: "color-mix(in srgb, var(--status-error) 75%, transparent)" }}
              >
                {failureInfo.failure_details}
              </p>
            )}
          </div>
        </section>
      )}

      {/* Agent Error Banner - shows last_agent_error for any status */}
      {agentError && (
        <section data-testid="agent-error-section" className="space-y-2">
          <SectionTitle>{agentError.contextLabel} Error</SectionTitle>
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
                    {getTimeAgo(agentError.errorAt)}
                  </span>
                )}
              </div>
            </div>
          </DetailCard>
        </section>
      )}

      {/* Duration (static) — shown when both timestamps exist */}
      {task.startedAt && task.completedAt && (
        <div data-testid="basic-task-duration">
          <DurationDisplay
            mode="static"
            startedAt={task.startedAt}
            completedAt={task.completedAt}
          />
        </div>
      )}

      {/* Steps Section */}
      {stepsLoading && (
        <div
          data-testid="basic-task-steps-loading"
          className="flex items-center justify-center py-8"
        >
          <Loader2
            className="w-5 h-5 animate-spin text-text-primary/30"
          />
        </div>
      )}

      {!stepsLoading && hasSteps && (
        <section data-testid="basic-task-steps-section">
          <SectionTitle>Steps</SectionTitle>
          <StepList taskId={task.id} editable={false} hideCompletionNotes={isHistorical} />
        </section>
      )}

      {!stepsLoading && !hasSteps && (
        <div className="text-[0.8125rem] text-text-secondary italic py-4">
          No steps defined yet
        </div>
      )}

      {/* Action Buttons (hidden in historical mode) */}
      {!isHistorical && showsActions && (
        <section>
          <ActionButtonsCard task={task} />
        </section>
      )}

      {/* Unblock action with warning (for blocked tasks with failed dependency) */}
      {!isHistorical && isBlockedByFailedDependency && failedDependencyName && (
        <section>
          <UnblockWarningCard task={task} failedDependencyName={failedDependencyName} />
        </section>
      )}
    </TwoColumnLayout>
  );
}
