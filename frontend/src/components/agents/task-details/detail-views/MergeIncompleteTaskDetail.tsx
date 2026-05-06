/**
 * MergeIncompleteTaskDetail - View for tasks in merge_incomplete state
 *
 * Shows error context for non-conflict git failures (branch deleted, git lock,
 * network failure), recovery steps, and action buttons for retry/resolve.
 * Uses error variant (red) to distinguish from MergeConflict's warning (amber).
 */

import { useState, useCallback } from "react";
import {
  AlertTriangle,
  CheckCircle2,
  Loader2,
  RefreshCw,
  SkipForward,
  Clock,
  PlayCircle,
  XCircle,
  CheckCircle,
  Ban,
  GitPullRequestClosed,
} from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  SectionTitle,
  DetailCard,
  StatusBanner,
  StatusPill,
  TwoColumnLayout,
} from "./shared";
import { ValidationProgress } from "./shared/ValidationProgress";
import type { Task, TaskMetadata, MergeRecoveryEvent } from "@/types/task";
import { useQueryClient } from "@tanstack/react-query";
import { taskKeys } from "@/hooks/useTasks";
import { extractErrorMessage } from "@/lib/errors";
import { useUiStore } from "@/stores/uiStore";
import { useConfirmation } from "@/hooks/useConfirmation";
import { api } from "@/lib/tauri";
import { BranchBadge, BranchFlow } from "@/components/shared/BranchBadge";
import { statusTint, withAlpha } from "@/lib/theme-colors";
import { useMergePipeline } from "@/hooks/useMergePipeline";
import { usePlanBranchForTask } from "@/hooks/usePlanBranchForTask";

interface MergeIncompleteTaskDetailProps {
  task: Task;
  isHistorical?: boolean;
}

interface MergeErrorContext {
  error: string | null;
  errorCode: string | null;
  sourceBranch: string | null;
  targetBranch: string | null;
  diagnosticInfo: string | null;
  hasValidationFailures: boolean;
  recoveryEvents: MergeRecoveryEvent[];
  hookFailureKind: string | null;
  hookBlockedReason: string | null;
  hookFailureRepeatCount: number | null;
  metadata: TaskMetadata | null;
}

const ATTEMPT_MESSAGE_PREVIEW_CHARS = 220;
const ERROR_CONTEXT_PREVIEW_CHARS = 900;

function buildAttemptMessagePreview(message: string): string {
  const condensed = message.replace(/\s+/g, " ").trim();
  if (condensed.length <= ATTEMPT_MESSAGE_PREVIEW_CHARS) {
    return condensed;
  }
  return `${condensed.slice(0, ATTEMPT_MESSAGE_PREVIEW_CHARS).trimEnd()}...`;
}

function buildErrorContextPreview(message: string): string {
  const trimmed = message.trim();
  if (trimmed.length <= ERROR_CONTEXT_PREVIEW_CHARS) {
    return trimmed;
  }
  return `${trimmed.slice(0, ERROR_CONTEXT_PREVIEW_CHARS).trimEnd()}...`;
}

function parseMergeError(metadata?: string | null): MergeErrorContext | null {
  if (!metadata) return null;
  try {
    const m: TaskMetadata = JSON.parse(metadata);
    return {
      error: m.error ?? null,
      errorCode: m.error_code ?? null,
      sourceBranch: m.source_branch ?? null,
      targetBranch: m.target_branch ?? null,
      diagnosticInfo: m.diagnostic_info ?? null,
      hasValidationFailures: Array.isArray(m.validation_failures) && m.validation_failures.length > 0,
      recoveryEvents: m.merge_recovery?.events ?? [],
      hookFailureKind: m.merge_hook_failure_kind ?? null,
      hookBlockedReason: m.merge_hook_blocked_reason ?? null,
      hookFailureRepeatCount: m.merge_hook_failure_repeat_count ?? null,
      metadata: m,
    };
  } catch {
    return null;
  }
}

function getHookBlockCopy(mergeError: MergeErrorContext | null): {
  title: string;
  subtitle: string;
  explanation: string;
} | null {
  if (mergeError?.hookBlockedReason === "hook_environment_failure") {
    return {
      title: "Escalated",
      subtitle: "Repository hook environment failed — action required",
      explanation:
        "The repository hook could not run reliably in this isolated worktree environment, so RalphX did not ask the agent to change code.",
    };
  }

  if (mergeError?.hookBlockedReason === "repeated_hook_failure") {
    return {
      title: "Escalated",
      subtitle: "Same repository hook failure repeated — loop stopped",
      explanation:
        "The same commit hook failure repeated after re-execution, so RalphX stopped the automatic revision loop.",
    };
  }

  return null;
}

/**
 * ErrorContextCard - Shows actual error details or generic fallback
 */
function ErrorContextCard({ mergeError, resolvedSource, resolvedTarget }: { mergeError: MergeErrorContext | null; resolvedSource?: string; resolvedTarget?: string | null }) {
  const [selectedErrorOutput, setSelectedErrorOutput] = useState<string | null>(null);
  const hookBlockCopy = getHookBlockCopy(mergeError);

  if (!mergeError) {
    return (
      <div className="space-y-3">
        <p className="text-[0.8125rem] text-text-primary/60">
          The merge failed due to a git error that is not a merge conflict.
          This can happen when:
        </p>
        <ul className="list-disc list-inside space-y-1.5 text-[0.8125rem] text-text-primary/50">
          <li>The task branch was deleted or corrupted</li>
          <li>A git lock file is preventing operations</li>
          <li>Network issues interrupted a fetch operation</li>
          <li>The worktree directory is missing or inaccessible</li>
        </ul>
      </div>
    );
  }

  const errorPreview = mergeError.error ? buildErrorContextPreview(mergeError.error) : null;
  const errorIsTruncated = errorPreview !== null && errorPreview !== mergeError.error?.trim();

  return (
    <>
      <div className="space-y-3">
        {hookBlockCopy && (
          <div className="rounded-md px-3 py-2 text-[0.8125rem] text-text-primary/70 bg-[var(--overlay-faint)]">
            {hookBlockCopy.explanation}
            {mergeError?.hookFailureRepeatCount != null && mergeError.hookFailureRepeatCount > 0 && (
              <span className="ml-1 text-text-primary/50">
                Repeat count: {mergeError.hookFailureRepeatCount}.
              </span>
            )}
          </div>
        )}
        {mergeError.error && (
          <div className="space-y-2">
            <div
              className="rounded-md px-3 py-2 font-mono text-[0.75rem] text-text-primary/80 whitespace-pre-wrap"
              style={{ backgroundColor: "var(--status-error-muted)" }}
            >
              {errorPreview}
            </div>
            {errorIsTruncated && (
              <button
                type="button"
                className="text-[0.75rem] font-medium text-[var(--accent-primary)] hover:text-[var(--accent-primary-hover)]"
                onClick={() => setSelectedErrorOutput(mergeError.error)}
              >
                View full output
              </button>
            )}
          </div>
        )}
        {(resolvedSource || resolvedTarget || mergeError.sourceBranch || mergeError.targetBranch) && (
          <div className="text-[0.8125rem] text-text-primary/60">
            <BranchFlow
              source={resolvedSource ?? mergeError.sourceBranch ?? "unknown"}
              target={resolvedTarget ?? mergeError.targetBranch ?? "unknown"}
            />
          </div>
        )}
        {mergeError.diagnosticInfo && (
          <div className="text-[0.75rem] text-text-primary/50 whitespace-pre-wrap">
            {mergeError.diagnosticInfo}
          </div>
        )}
      </div>

      <Dialog
        open={selectedErrorOutput !== null}
        onOpenChange={(open) => {
          if (!open) {
            setSelectedErrorOutput(null);
          }
        }}
      >
        <DialogContent
          data-testid="merge-error-context-dialog"
          className="sm:max-w-3xl max-h-[80vh] overflow-hidden"
        >
          <DialogHeader>
            <DialogTitle>Full error output</DialogTitle>
            <DialogDescription>
              Full merge error output in a scrollable view.
            </DialogDescription>
          </DialogHeader>
          <div className="px-6 pb-6">
            <div className="max-h-[56vh] overflow-y-auto rounded-lg bg-[var(--overlay-faint)] p-4">
              <pre className="whitespace-pre-wrap break-words font-mono text-[0.75rem] text-text-primary/80">
                {selectedErrorOutput}
              </pre>
            </div>
          </div>
        </DialogContent>
      </Dialog>
    </>
  );
}

/**
 * RecoverySteps - Numbered steps for manual recovery
 */
function RecoverySteps({
  branchName,
  targetBranch,
  hasValidationFailures,
  hookBlockedReason,
}: {
  branchName: string;
  targetBranch?: string | null;
  hasValidationFailures: boolean;
  hookBlockedReason?: string | null;
}) {
  return (
    <div className="space-y-3">
      {hookBlockedReason === "hook_environment_failure" ? (
        <>
          <p className="text-[0.8125rem] text-text-primary/60">
            A repository commit hook could not bootstrap its environment in the merge worktree.
            Fix the hook dependencies or worktree setup, then retry the merge.
          </p>
          <ol className="list-decimal list-inside space-y-2 text-[0.8125rem] text-text-primary/50">
            <li>Check the hook output for missing tools, dependencies, permissions, or symlinks</li>
            <li>Repair the worktree setup or install the missing dependencies outside the task agent flow</li>
            <li>
              Click <strong className="text-text-primary/70">Retry after environment fix</strong> after the environment is fixed
            </li>
          </ol>
        </>
      ) : hookBlockedReason === "repeated_hook_failure" ? (
        <>
          <p className="text-[0.8125rem] text-text-primary/60">
            The same repository hook failure repeated after re-execution, so RalphX stopped the automatic loop.
          </p>
          <ol className="list-decimal list-inside space-y-2 text-[0.8125rem] text-text-primary/50">
            <li>Review the full hook output to decide whether this is code feedback or environment setup</li>
            <li>Fix the root cause manually or update the hook/worktree setup</li>
            <li>
              Click <strong className="text-text-primary/70">Retry after fix</strong> only after the cause is addressed
            </li>
          </ol>
        </>
      ) : hasValidationFailures ? (
        <>
          <p className="text-[0.8125rem] text-text-primary/60">
            Your validation commands (build, type checks, linting) failed,
            so the merge could not be completed. To recover:
          </p>
          <ol className="list-decimal list-inside space-y-2 text-[0.8125rem] text-text-primary/50">
            <li>
              Fix the build, type, or lint errors in your codebase
            </li>
            <li>
              Click <strong className="text-text-primary/70">Retry Merge</strong> to
              re-run validation and complete the merge
            </li>
            <li>
              Click{" "}
              <strong className="text-text-primary/70">Retry (Skip Validation)</strong>{" "}
              to complete the merge without running validation
            </li>
            <li>
              If fixed manually, click{" "}
              <strong className="text-text-primary/70">Mark Resolved</strong>
            </li>
          </ol>
        </>
      ) : (
        <>
          <p className="text-[0.8125rem] text-text-primary/60">
            To recover, try the following steps:
          </p>
          <ol className="list-decimal list-inside space-y-2 text-[0.8125rem] text-text-primary/50">
            <li>
              Check if the branch exists:{" "}
              <code className="text-text-primary/70 bg-[var(--overlay-faint)] px-1 rounded">
                git branch --list {branchName}
              </code>
            </li>
            <li>
              Remove any stale lock files:{" "}
              <code className="text-text-primary/70 bg-[var(--overlay-faint)] px-1 rounded">
                rm -f .git/index.lock
              </code>
            </li>
            <li>
              Click <strong className="text-text-primary/70">Retry Merge</strong> to
              attempt the merge again
            </li>
            <li>
              If the issue is resolved manually, click{" "}
              <strong className="text-text-primary/70">Mark Resolved</strong>
            </li>
          </ol>
        </>
      )}
      <div className="pt-2">
        {targetBranch ? (
          <BranchFlow source={branchName} target={targetBranch} size="sm" />
        ) : (
          <BranchBadge branch={branchName} variant="muted" size="sm" />
        )}
      </div>
    </div>
  );
}

/**
 * RecoveryTimeline - Shows chronological timeline of merge recovery attempts
 */
function RecoveryTimeline({ events }: { events: MergeRecoveryEvent[] }) {
  const [selectedMessage, setSelectedMessage] = useState<{
    title: string;
    message: string;
  } | null>(null);

  const formatTimestamp = (isoString: string) => {
    try {
      const date = new Date(isoString);
      return date.toLocaleString("en-US", {
        month: "short",
        day: "numeric",
        hour: "2-digit",
        minute: "2-digit",
        second: "2-digit",
      });
    } catch {
      return isoString;
    }
  };

  const getEventIcon = (kind: string) => {
    switch (kind) {
      case "deferred":
        return Clock;
      case "auto_retry_triggered":
        return RefreshCw;
      case "attempt_started":
        return PlayCircle;
      case "attempt_failed":
        return XCircle;
      case "attempt_succeeded":
        return CheckCircle;
      case "manual_retry":
        return RefreshCw;
      default:
        return Clock;
    }
  };

  const getEventColor = (kind: string) => {
    switch (kind) {
      case "deferred":
        return statusTint("warning", 70); // amber
      case "auto_retry_triggered":
      case "manual_retry":
        return statusTint("accent", 70); // orange
      case "attempt_started":
        return statusTint("info", 70); // blue
      case "attempt_failed":
        return statusTint("error", 70); // red
      case "attempt_succeeded":
        return "var(--status-success)"; // green
      default:
        return withAlpha("var(--text-primary)", 50); // white/gray
    }
  };

  const getKindLabel = (kind: string) => {
    switch (kind) {
      case "deferred":
        return "Deferred";
      case "auto_retry_triggered":
        return "Auto-retry Triggered";
      case "attempt_started":
        return "Attempt Started";
      case "attempt_failed":
        return "Attempt Failed";
      case "attempt_succeeded":
        return "Succeeded";
      case "manual_retry":
        return "Manual Retry";
      default:
        return kind;
    }
  };

  const getSourceBadge = (source: string) => {
    const colors = {
      system: "var(--status-info-muted)",
      auto: "var(--accent-muted)",
      user: "var(--status-success-muted)",
    };
    return (
      <span
        className="text-[0.625rem] px-2 py-0.5 rounded-full font-medium uppercase tracking-wide text-text-primary/70"
        style={{
          backgroundColor: colors[source as keyof typeof colors] ?? "var(--overlay-moderate)",
        }}
      >
        {source}
      </span>
    );
  };

  return (
    <>
      <div className="space-y-3">
        {events.map((event, idx) => {
        const Icon = getEventIcon(event.kind);
        const color = getEventColor(event.kind);
        const preview = buildAttemptMessagePreview(event.message);
        const isTruncated = preview !== event.message.replace(/\s+/g, " ").trim();

        return (
          <div
            key={idx}
            className="flex gap-3 pb-3"
            style={{
              borderBottom:
                idx < events.length - 1
                  ? "1px solid var(--overlay-weak)"
                  : "none",
            }}
          >
            {/* Icon */}
            <div
              className="flex-shrink-0 w-8 h-8 rounded-full flex items-center justify-center"
              style={{ backgroundColor: `${color}20` }}
            >
              <Icon className="w-4 h-4" style={{ color }} />
            </div>

            {/* Content */}
            <div className="flex-1 space-y-1.5">
              {/* Header: kind + timestamp */}
              <div className="flex items-center justify-between gap-2">
                <div className="flex items-center gap-2 flex-wrap">
                  <span className="text-[0.8125rem] font-medium text-text-primary/90">
                    {getKindLabel(event.kind)}
                  </span>
                  {getSourceBadge(event.source)}
                  {event.attempt !== undefined && (
                    <span className="text-[0.6875rem] text-text-primary/40">
                      Attempt #{event.attempt}
                    </span>
                  )}
                </div>
                <span className="text-[0.6875rem] text-text-primary/40 font-mono">
                  {formatTimestamp(event.at)}
                </span>
              </div>

              {/* Message */}
              <div className="space-y-1.5">
                <p className="text-[0.8125rem] text-text-primary/70 break-words">{preview}</p>
                {isTruncated && (
                  <button
                    type="button"
                    className="text-[0.75rem] font-medium text-[var(--accent-primary)] hover:text-[var(--accent-primary-hover)]"
                    onClick={() =>
                      setSelectedMessage({
                        title: getKindLabel(event.kind),
                        message: event.message,
                      })
                    }
                  >
                    View full output
                  </button>
                )}
              </div>

              {/* Additional metadata */}
              <div className="flex flex-wrap gap-x-4 gap-y-1 text-[0.6875rem] text-text-primary/50">
                {event.blocking_task_id && (
                  <div>
                    <span className="text-text-primary/40">Blocker: </span>
                    <span className="font-mono">{event.blocking_task_id.slice(0, 8)}</span>
                  </div>
                )}
                {event.target_branch && (
                  <div>
                    <span className="text-text-primary/40">Target: </span>
                    <span className="font-mono">{event.target_branch}</span>
                  </div>
                )}
                {event.reason_code && (
                  <div>
                    <span className="text-text-primary/40">Reason: </span>
                    <span>{event.reason_code.replace(/_/g, " ")}</span>
                  </div>
                )}
              </div>
            </div>
          </div>
        );
        })}
      </div>

      <Dialog
        open={selectedMessage !== null}
        onOpenChange={(open) => {
          if (!open) {
            setSelectedMessage(null);
          }
        }}
      >
        <DialogContent
          data-testid="merge-attempt-message-dialog"
          className="sm:max-w-3xl max-h-[80vh] overflow-hidden"
        >
          <DialogHeader>
            <DialogTitle>{selectedMessage?.title ?? "Attempt output"}</DialogTitle>
            <DialogDescription>
              Full merge attempt output in a scrollable view.
            </DialogDescription>
          </DialogHeader>
          <div className="px-6 pb-6">
            <div className="max-h-[56vh] overflow-y-auto rounded-lg bg-[var(--overlay-faint)] p-4">
              <pre className="whitespace-pre-wrap break-words font-mono text-[0.75rem] text-text-primary/80">
                {selectedMessage?.message}
              </pre>
            </div>
          </div>
        </DialogContent>
      </Dialog>
    </>
  );
}

/**
 * RecoveryBadges - Show status badges based on recovery state
 */
function RecoveryBadges({
  hasAutoRetry,
  hasDeferred,
  lastAttemptFailed,
}: {
  hasAutoRetry: boolean;
  hasDeferred: boolean;
  lastAttemptFailed: boolean;
}) {
  return (
    <div className="flex gap-2 flex-wrap">
      {hasAutoRetry && (
        <span
          className="text-[0.6875rem] px-2.5 py-1 rounded-full font-medium"
          style={{
            backgroundColor: "var(--accent-muted)",
            color: "var(--accent-primary)",
          }}
        >
          Auto-recovery attempted
        </span>
      )}
      {hasDeferred && (
        <span
          className="text-[0.6875rem] px-2.5 py-1 rounded-full font-medium"
          style={{
            backgroundColor: "var(--status-warning-muted)",
            color: "var(--status-warning)",
          }}
        >
          Deferred due to active merge
        </span>
      )}
      {lastAttemptFailed && (
        <span
          className="text-[0.6875rem] px-2.5 py-1 rounded-full font-medium"
          style={{
            backgroundColor: "var(--status-error-muted)",
            color: "var(--status-error)",
          }}
        >
          Last attempt failed
        </span>
      )}
    </div>
  );
}

/**
 * ActionButtons - Retry Merge (primary) + Mark Resolved (green) + Cancel (red)
 */
function ActionButtons({
  onRetry,
  onRetrySkipValidation,
  onResolve,
  onCancel,
  isProcessing,
  retryLabel = "Retry Merge",
}: {
  onRetry: () => void;
  onRetrySkipValidation?: (() => void) | undefined;
  onResolve: () => void;
  onCancel: () => void;
  isProcessing: boolean;
  retryLabel?: string;
}) {
  return (
    <div className="flex gap-2 justify-end flex-wrap">
      <Button
        data-testid="retry-merge-button"
        onClick={onRetry}
        disabled={isProcessing}
        className="h-9 px-4 gap-2 rounded-lg font-medium text-[0.8125rem]"
        style={{
          color: "white",
          backgroundColor: "var(--accent-primary)",
        }}
      >
        {isProcessing ? (
          <Loader2 className="w-4 h-4 animate-spin" />
        ) : (
          <RefreshCw className="w-4 h-4" />
        )}
        {retryLabel}
      </Button>
      {onRetrySkipValidation && (
        <Button
          data-testid="retry-skip-validation-button"
          onClick={onRetrySkipValidation}
          disabled={isProcessing}
          className="h-9 px-4 gap-2 rounded-lg font-medium text-[0.8125rem]"
          style={{
            color: "white",
            backgroundColor: statusTint("warning", 85),
          }}
        >
          {isProcessing ? (
            <Loader2 className="w-4 h-4 animate-spin" />
          ) : (
            <SkipForward className="w-4 h-4" />
          )}
          Retry (Skip Validation)
        </Button>
      )}
      <Button
        data-testid="resolve-merge-button"
        onClick={onResolve}
        disabled={isProcessing}
        className="h-9 px-4 gap-2 rounded-lg font-medium text-[0.8125rem]"
        style={{
          color: "white",
          backgroundColor: "var(--status-success)",
        }}
      >
        {isProcessing ? (
          <Loader2 className="w-4 h-4 animate-spin" />
        ) : (
          <CheckCircle2 className="w-4 h-4" />
        )}
        Mark Resolved
      </Button>
      <Button
        data-testid="cancel-task-button"
        onClick={onCancel}
        disabled={isProcessing}
        className="h-9 px-4 gap-2 rounded-lg font-medium text-[0.8125rem]"
        style={{
          color: "white",
          backgroundColor: "#ff4545",
        }}
      >
        {isProcessing ? (
          <Loader2 className="w-4 h-4 animate-spin" />
        ) : (
          <Ban className="w-4 h-4" />
        )}
        Cancel
      </Button>
    </div>
  );
}

export function MergeIncompleteTaskDetail({
  task,
  isHistorical = false,
}: MergeIncompleteTaskDetailProps) {
  const queryClient = useQueryClient();
  const setHistoryState = useUiStore((state) => state.setTaskHistoryState);
  const [isProcessing, setIsProcessing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { confirm } = useConfirmation();

  const mergeError = parseMergeError(task.metadata);
  const hookBlockCopy = getHookBlockCopy(mergeError);
  const isHookEscalation = mergeError?.hookBlockedReason === "hook_environment_failure"
    || mergeError?.hookBlockedReason === "repeated_hook_failure";
  const { data: planBranch } = usePlanBranchForTask(task.id);

  // Use merge pipeline data for correct branch resolution (metadata may have stale target_branch)
  const { data: pipelineData } = useMergePipeline(task.projectId);
  const pipelineTask = pipelineData?.needsAttention.find((t) => t.taskId === task.id);
  const resolvedSourceBranch = pipelineTask?.sourceBranch ?? mergeError?.sourceBranch ?? task.taskBranch ?? "task branch";
  const resolvedTargetBranch = pipelineTask?.targetBranch ?? mergeError?.targetBranch ?? null;

  const branchName = resolvedSourceBranch;

  // Derive recovery state badges from events
  const hasAutoRetryAttempts = mergeError?.recoveryEvents.some(
    (e) => e.kind === "auto_retry_triggered" || e.source === "auto"
  );
  const hasDeferred = mergeError?.recoveryEvents.some((e) => e.kind === "deferred");
  const lastEvent = mergeError?.recoveryEvents[mergeError.recoveryEvents.length - 1];
  const lastAttemptFailed = lastEvent?.kind === "attempt_failed";

  const handleRetryMerge = useCallback(async () => {
    setIsProcessing(true);
    setError(null);

    // Exit history mode to show live view
    setHistoryState(null);

    // Optimistically update task status to pending_merge
    queryClient.setQueryData<Task[]>(
      taskKeys.list(task.projectId),
      (old) => old?.map((t) =>
        t.id === task.id ? { ...t, internalStatus: "pending_merge" as const } : t
      )
    );

    try {
      await invoke("retry_merge", { taskId: task.id });
      await queryClient.invalidateQueries({
        queryKey: taskKeys.list(task.projectId),
      });
    } catch (err) {
      setError(extractErrorMessage(err, "Failed to retry merge"));
      // Rollback optimistic update on error
      await queryClient.invalidateQueries({
        queryKey: taskKeys.list(task.projectId),
      });
    } finally {
      setIsProcessing(false);
    }
  }, [task.id, task.projectId, queryClient, setHistoryState]);

  const handleRetrySkipValidation = useCallback(async () => {
    setIsProcessing(true);
    setError(null);

    // Exit history mode to show live view
    setHistoryState(null);

    // Optimistically update task status to pending_merge
    queryClient.setQueryData<Task[]>(
      taskKeys.list(task.projectId),
      (old) => old?.map((t) =>
        t.id === task.id ? { ...t, internalStatus: "pending_merge" as const } : t
      )
    );

    try {
      await invoke("retry_merge", { taskId: task.id, skipValidation: true });
      await queryClient.invalidateQueries({
        queryKey: taskKeys.list(task.projectId),
      });
    } catch (err) {
      setError(
        extractErrorMessage(err, "Failed to retry merge (skipping validation)"),
      );
      // Rollback optimistic update on error
      await queryClient.invalidateQueries({
        queryKey: taskKeys.list(task.projectId),
      });
    } finally {
      setIsProcessing(false);
    }
  }, [task.id, task.projectId, queryClient, setHistoryState]);

  const handleMarkResolved = useCallback(async () => {
    setIsProcessing(true);
    setError(null);
    try {
      await invoke("resolve_merge_conflict", { taskId: task.id });
      await queryClient.invalidateQueries({
        queryKey: taskKeys.list(task.projectId),
      });
    } catch (err) {
      setError(
        extractErrorMessage(err, "Failed to mark merge as resolved"),
      );
    } finally {
      setIsProcessing(false);
    }
  }, [task.id, task.projectId, queryClient]);

  const handleCancel = useCallback(async () => {
    const confirmed = await confirm({
      title: "Cancel task?",
      description: "This will transition the task to Cancelled status. This action cannot be undone.",
      confirmText: "Cancel",
      variant: "destructive",
    });

    if (!confirmed) return;

    setIsProcessing(true);
    setError(null);
    try {
      await api.tasks.move(task.id, "cancelled");
      await queryClient.invalidateQueries({
        queryKey: taskKeys.list(task.projectId),
      });
    } catch (err) {
      setError(extractErrorMessage(err, "Failed to cancel task"));
    } finally {
      setIsProcessing(false);
    }
  }, [task.id, task.projectId, queryClient, confirm]);

  return (
    <TwoColumnLayout
      description={task.description}
      testId="merge-incomplete-task-detail"
    >
      {/* Status Banner - error (red) variant */}
      <StatusBanner
        icon={AlertTriangle}
        title={hookBlockCopy?.title ?? "Merge Incomplete"}
        subtitle={
          hookBlockCopy
            ? hookBlockCopy.subtitle
            : mergeError?.hasValidationFailures
            ? isHistorical
              ? "Merge validation failed"
              : "Merge validation failed — action required"
            : isHistorical
              ? "A git error prevented the merge"
              : "A git error prevented the merge — action required"
        }
        variant={isHookEscalation ? "warning" : "error"}
        badge={
          <StatusPill
            icon={AlertTriangle}
            label={isHookEscalation ? "Escalated" : "Error"}
            variant={isHookEscalation ? "warning" : "error"}
            size="md"
          />
        }
      />

      {/* PR Context Banner — shown when failure is PR-related */}
      {planBranch?.prEligible && (
        <section data-testid="pr-context-section">
          {planBranch.prStatus === "Closed" ? (
            <DetailCard variant="warning">
              <div className="flex items-center gap-2">
                <GitPullRequestClosed className="w-4 h-4" style={{ color: "var(--status-warning)" }} />
                <span className="text-[0.8125rem] text-text-primary/70">PR closed without merging</span>
                {planBranch.prNumber != null && (
                  <span className="text-[0.75rem] text-text-primary/40">PR #{planBranch.prNumber}</span>
                )}
              </div>
            </DetailCard>
          ) : planBranch.prNumber != null ? (
            <DetailCard variant="error">
              <div className="flex items-center gap-2">
                <AlertTriangle className="w-4 h-4" style={{ color: "var(--status-error)" }} />
                <span className="text-[0.8125rem] text-text-primary/70">
                  PR operation failed (PR #{planBranch.prNumber})
                </span>
              </div>
            </DetailCard>
          ) : (
            <DetailCard variant="error">
              <div className="flex items-center gap-2">
                <AlertTriangle className="w-4 h-4" style={{ color: "var(--status-error)" }} />
                <span className="text-[0.8125rem] text-text-primary/70">PR operation failed</span>
              </div>
            </DetailCard>
          )}
        </section>
      )}

      {/* Recovery Attempts Timeline or Fallback */}
      <section data-testid="recovery-attempts-section">
        <SectionTitle>Recovery Attempts</SectionTitle>
        {mergeError && mergeError.recoveryEvents.length > 0 ? (
          <>
            <DetailCard>
              <RecoveryTimeline events={mergeError.recoveryEvents} />
            </DetailCard>
            {/* Recovery Status Badges */}
            <div className="mt-3">
              <RecoveryBadges
                hasAutoRetry={hasAutoRetryAttempts ?? false}
                hasDeferred={hasDeferred ?? false}
                lastAttemptFailed={lastAttemptFailed ?? false}
              />
            </div>
          </>
        ) : (
          <DetailCard>
            <p className="text-[0.8125rem] text-text-primary/50 italic">
              No recorded recovery attempts for this task.
            </p>
          </DetailCard>
        )}
      </section>

      {/* Error Context */}
      <section data-testid="error-context-section">
        <SectionTitle>What Happened</SectionTitle>
        <DetailCard variant="error">
          <ErrorContextCard mergeError={mergeError} resolvedSource={resolvedSourceBranch} resolvedTarget={resolvedTargetBranch} />
        </DetailCard>
      </section>

      {/* Validation Log (when failure was caused by validation) */}
      {mergeError?.hasValidationFailures && (
        <ValidationProgress
          taskId={task.id}
          metadata={task.metadata}
        />
      )}

      {/* Recovery Steps (not in historical mode) */}
      {!isHistorical && (
        <section data-testid="recovery-steps-section">
          <SectionTitle>How to Recover</SectionTitle>
          <DetailCard>
            <RecoverySteps
              branchName={branchName}
              targetBranch={resolvedTargetBranch}
              hasValidationFailures={mergeError?.hasValidationFailures ?? false}
              hookBlockedReason={mergeError?.hookBlockedReason ?? null}
            />
          </DetailCard>
        </section>
      )}

      {/* Error Display */}
      {error && (
        <div
          className="p-3 rounded-lg text-[0.8125rem]"
          style={{
            backgroundColor: statusTint("error", 12),
            color: "var(--status-error)",
          }}
        >
          {error}
        </div>
      )}

      {/* Actions (hidden in historical mode) */}
      {!isHistorical && (
        <section data-testid="action-buttons">
          <ActionButtons
            onRetry={handleRetryMerge}
            onRetrySkipValidation={mergeError?.hasValidationFailures ? handleRetrySkipValidation : undefined}
            onResolve={handleMarkResolved}
            onCancel={handleCancel}
            isProcessing={isProcessing}
            retryLabel={
              mergeError?.hookBlockedReason === "hook_environment_failure"
                ? "Retry after environment fix"
                : mergeError?.hookBlockedReason === "repeated_hook_failure"
                  ? "Retry after fix"
                  : "Retry Merge"
            }
          />
        </section>
      )}
    </TwoColumnLayout>
  );
}
