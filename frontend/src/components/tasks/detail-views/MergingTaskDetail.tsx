/**
 * MergingTaskDetail - Handles both pending_merge and merging states
 *
 * pending_merge: Programmatic merge attempt in progress (Phase 1)
 * merging: Agent-assisted conflict resolution in progress (Phase 2)
 *
 * This combined view provides a seamless UX during the merge process.
 * PendingMerge is typically very brief (1-3 seconds).
 */

import { useMemo, useState, useCallback } from "react";
import {
  Loader2,
  GitMerge,
  AlertTriangle,
  Bot,
  FileWarning,
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  Wrench,
  RefreshCw,
  Square,
  Info,
  ExternalLink,
  GitPullRequest,
} from "lucide-react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useConflictDiff } from "@/hooks/useConflictDiff";
import { ConflictDiffViewer } from "@/components/diff/ConflictDiffViewer";
import {
  SectionTitle,
  DetailCard,
  StatusBanner,
  StatusPill,
  TwoColumnLayout,
  ChangeReviewSection,
  PlanMergeContextSection,
} from "./shared";
import { useTaskDetailContextModel } from "./shared/TaskDetailContext";
import { useMergeValidationEvents } from "@/hooks/useMergeValidationEvents";
import { useMergeProgressEvents } from "@/hooks/useMergeProgressEvents";
import { useConflictDetection } from "@/hooks/useConflictDetection";
import { MergePhaseTimeline } from "./MergePhaseTimeline";
import { ValidationProgress } from "./shared/ValidationProgress";
import { useConfirmation } from "@/hooks/useConfirmation";
import { api } from "@/lib/tauri";
import { taskKeys } from "@/hooks/useTasks";
import type { Task } from "@/types/task";
import { BranchBadge, BranchFlow } from "@/components/shared/BranchBadge";
import { useMergePipeline } from "@/hooks/useMergePipeline";
import { usePlanBranchForTask } from "@/hooks/usePlanBranchForTask";
import { PrStatusBadge } from "./shared/PrStatusBadge";
import { statusTint, withAlpha } from "@/lib/theme-colors";
import { useTaskStateHistory } from "@/hooks/useReviews";
import { useTaskStateTransitions } from "@/hooks/useTaskStateTransitions";

interface MergingTaskDetailProps {
  task: Task;
  isHistorical?: boolean;
  viewStatus?: string | undefined;
}

/**
 * ConflictFilesList - Shows files with merge conflicts, expandable to show diff
 */
function ConflictFilesList({
  files,
  taskId,
}: {
  files: string[];
  taskId: string;
}) {
  const [expandedFile, setExpandedFile] = useState<string | null>(null);
  const { data: conflictDiff, isLoading: isLoadingDiff } = useConflictDiff({
    taskId,
    filePath: expandedFile,
  });

  if (files.length === 0) return null;

  const toggleFile = (file: string) => {
    setExpandedFile(expandedFile === file ? null : file);
  };

  return (
    <div className="space-y-2">
      {files.map((file, index) => (
        <div key={index}>
          <button
            type="button"
            onClick={() => toggleFile(file)}
            className="w-full flex items-center gap-2 py-2 px-3 rounded-lg transition-colors cursor-pointer"
            style={{
              backgroundColor:
                expandedFile === file
                  ? "var(--status-warning-muted)"
                  : "var(--status-warning-muted)",
            }}
          >
            {expandedFile === file ? (
              <ChevronDown
                className="w-4 h-4 shrink-0"
                style={{ color: "var(--status-warning)" }}
              />
            ) : (
              <ChevronRight
                className="w-4 h-4 shrink-0"
                style={{ color: "var(--status-warning)" }}
              />
            )}
            <FileWarning className="w-4 h-4 shrink-0" style={{ color: "var(--status-warning)" }} />
            <span
              className="text-[0.8125rem] font-mono text-text-primary/70 truncate text-left"
              title={file}
            >
              {file}
            </span>
          </button>
          {expandedFile === file && (
            <div
              className="mt-2 rounded-lg overflow-hidden border"
              style={{
                borderColor: "var(--status-warning-border)",
                height: "400px",
              }}
            >
              {isLoadingDiff ? (
                <div
                  className="flex items-center justify-center h-full"
                  style={{ backgroundColor: "var(--bg-base)" }}
                >
                  <Loader2 className="w-5 h-5 animate-spin text-text-primary/50" />
                </div>
              ) : conflictDiff ? (
                <ConflictDiffViewer conflictDiff={conflictDiff} />
              ) : (
                <div
                  className="flex items-center justify-center h-full text-text-primary/50"
                  style={{ backgroundColor: "var(--bg-base)" }}
                >
                  Failed to load conflict diff
                </div>
              )}
            </div>
          )}
        </div>
      ))}
    </div>
  );
}

/**
 * MergeStepIcon - Renders the icon for a single merge step
 */
function MergeStepIcon({ status, isHistorical }: { status: "completed" | "active" | "pending"; isHistorical?: boolean | undefined }) {
  if (status === "completed") {
    return <CheckCircle2 className="w-5 h-5" style={{ color: "var(--status-success)" }} />;
  }
  if (status === "active" && !isHistorical) {
    return (
      <div className="relative">
        <Loader2 className="w-5 h-5 animate-spin" style={{ color: "var(--status-info)" }} />
        <div
          className="absolute inset-0 rounded-full animate-pulse"
          style={{ background: "radial-gradient(circle, var(--status-info-border) 0%, transparent 70%)" }}
        />
      </div>
    );
  }
  if (status === "active" && isHistorical) {
    return (
      <div
        className="w-5 h-5 rounded-full border-2 border-text-primary/20"
        style={{ backgroundColor: "var(--status-warning-strong)" }}
      />
    );
  }
  return (
    <div className="w-5 h-5 rounded-full border-2 border-text-primary/20" />
  );
}

/**
 * MergeProgressSteps - Collapsible progress through merge phases.
 * Shows only the active step collapsed; expand to see all steps.
 *
 * Rendered for: historical modes, live agent phase, validation recovery (fixing).
 * NOT rendered during: live programmatic merge or revalidation (MergePhaseTimeline handles those).
 */
function MergeProgressSteps({
  isProgrammaticPhase,
  isHistorical,
  historicalMode,
  isValidationRecovery,
  isValidating,
  isRevalidating,
}: {
  isProgrammaticPhase: boolean;
  isHistorical?: boolean | undefined;
  historicalMode?: "attempted" | "resolving" | "waiting_on_pr" | undefined;
  isValidationRecovery?: boolean;
  isValidating?: boolean;
  isRevalidating?: boolean;
}) {
  const [expanded, setExpanded] = useState(false);

  type StepStatus = "completed" | "active" | "pending";
  const steps: { label: string; status: StepStatus }[] = isValidationRecovery && !isProgrammaticPhase
    ? isRevalidating
      ? [
          { label: "Merge completed", status: "completed" },
          { label: "Validation failed", status: "completed" },
          { label: "AI agent fixing build errors", status: "completed" },
          { label: "Re-validating fixes", status: isHistorical ? "completed" : "active" },
        ]
      : [
          { label: "Merge completed", status: "completed" },
          { label: "Validation failed", status: "completed" },
          { label: "AI agent fixing build errors", status: isHistorical ? "completed" : "active" },
          { label: "Re-validating fixes", status: "pending" },
        ]
    : isHistorical
    ? historicalMode === "attempted"
      ? [
          { label: "Merging branches", status: "completed" },
          { label: "Running validation", status: "pending" },
        ]
      : historicalMode === "waiting_on_pr"
      ? [
          { label: "Waiting on pull request", status: "completed" },
        ]
      : [
          { label: "Merging branches", status: "completed" },
          { label: "Agent resolving conflicts", status: "active" },
        ]
    : isProgrammaticPhase
    ? isValidating
      ? [
          { label: "Merging branches", status: "completed" },
          { label: "Running validation", status: "active" },
          { label: "Completing merge", status: "pending" },
        ]
      : [
          { label: "Merging branches", status: "active" },
          { label: "Running validation", status: "pending" },
          { label: "Completing merge", status: "pending" },
        ]
    : [
        { label: "Merging branches", status: "completed" },
        { label: "Agent resolving conflicts", status: "active" },
        { label: "Completing merge", status: "pending" },
      ];

  const activeStep = steps.find((s) => s.status === "active");
  const displayStep = activeStep ?? steps[steps.length - 1];

  return (
    <div>
      {/* Collapsed: show only the active step */}
      <button
        type="button"
        className="w-full flex items-center gap-3 px-3 py-3 text-left cursor-pointer"
        onClick={() => setExpanded(!expanded)}
      >
        <div className="relative">
          {displayStep !== undefined && <MergeStepIcon status={displayStep.status} isHistorical={isHistorical} />}
        </div>
        <span className="text-[0.8125rem] font-medium flex-1 text-text-primary/60">
          {displayStep?.label ?? "Merge"}
        </span>
        {expanded
          ? <ChevronDown className="w-4 h-4 text-text-primary/30 shrink-0" />
          : <ChevronRight className="w-4 h-4 text-text-primary/30 shrink-0" />}
      </button>

      {/* Expanded: show all steps in original style */}
      {expanded && (
        <div>
          {steps.map((step, index) => (
            <div
              key={index}
              className="flex items-center gap-3 px-3 py-3"
              style={
                index < steps.length - 1
                  ? { borderBottom: "1px solid var(--border-subtle)" }
                  : undefined
              }
            >
              <div className="relative">
                <MergeStepIcon status={step.status} isHistorical={isHistorical} />
              </div>
              <span
                className="text-[0.8125rem] font-medium"
                style={{
                  color: step.status === "completed"
                    ? withAlpha("var(--text-primary)", 60)
                    : step.status === "active"
                    ? isHistorical ? withAlpha("var(--text-primary)", 35) : "var(--status-info)"
                    : withAlpha("var(--text-primary)", 35),
                }}
              >
                {step.label}
              </span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

/**
 * PrModeCard - Shows PR status when task is in PR polling mode
 */
function PrModeCard({
  planBranch,
  isHistorical = false,
}: {
  planBranch: {
    prNumber: number;
    prUrl: string | null;
    prStatus: "Draft" | "Open" | "Merged" | "Closed" | null;
    prPollingActive: boolean;
  };
  isHistorical?: boolean;
}) {
  const liveStatusText =
    planBranch.prStatus === "Draft"
      ? "Draft PR published. RalphX will keep the branch synced until it is ready."
      : planBranch.prStatus === "Open"
      ? "Waiting for GitHub review or merge."
      : "Watching GitHub for PR updates.";
  const statusText = isHistorical
    ? "At this point, RalphX was waiting for GitHub review or merge."
    : liveStatusText;
  const showStatusText = isHistorical || planBranch.prPollingActive;

  const handleOpenInGithub = async () => {
    if (planBranch.prUrl) {
      const { openUrl } = await import("@tauri-apps/plugin-opener");
      await openUrl(planBranch.prUrl);
    }
  };

  return (
    <DetailCard>
      <div className="space-y-3">
        {/* PR Number + Status */}
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <GitPullRequest className="w-4 h-4" style={{ color: "var(--status-success)" }} />
            <span className="text-[0.8125rem] font-medium text-text-primary/80">
              PR #{planBranch.prNumber}
            </span>
          </div>
          {planBranch.prStatus && <PrStatusBadge status={planBranch.prStatus} />}
        </div>

        {/* Polling / historical state note */}
        {showStatusText && (
          <div className="flex items-center gap-1.5">
            {isHistorical ? (
              <Info className="w-3.5 h-3.5 text-text-primary/35" />
            ) : (
              <Loader2
                className="w-3.5 h-3.5 animate-spin text-text-primary/40"
              />
            )}
            <span className="text-[0.75rem] text-text-primary/40">{statusText}</span>
          </div>
        )}

        {/* Open in GitHub button */}
        {planBranch.prUrl && (
          <button
            type="button"
            onClick={handleOpenInGithub}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-md text-[0.75rem] font-medium transition-colors cursor-pointer"
            style={{
              backgroundColor: statusTint("success", 12),
              color: "var(--status-success)",
            }}
          >
            <ExternalLink className="w-3.5 h-3.5" />
            Open in GitHub
          </button>
        )}
      </div>
    </DetailCard>
  );
}

export function MergingTaskDetail({ task, isHistorical, viewStatus }: MergingTaskDetailProps) {
  // Action buttons state
  const queryClient = useQueryClient();
  const { confirm, confirmationDialogProps, ConfirmationDialog } = useConfirmation();
  const [actionError, setActionError] = useState<string | null>(null);
  const detailContext = useTaskDetailContextModel();

  const stopMutation = useMutation({
    mutationFn: async () => {
      await api.tasks.stop(task.id);
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: taskKeys.all });
      setActionError(null);
    },
    onError: (err) => {
      setActionError(err instanceof Error ? err.message : "Failed to stop merge");
    },
  });

  const handleStop = useCallback(async () => {
    const confirmed = await confirm({
      title: "Stop merge?",
      description: "This will stop the merge agent and leave the task in its current state.",
      confirmText: "Stop Merge",
      variant: "destructive",
    });
    if (!confirmed) return;
    stopMutation.mutate();
  }, [confirm, stopMutation]);

  const status = isHistorical && viewStatus ? viewStatus : task.internalStatus;
  const isPlanMerge = task.category === "plan_merge";
  const isProgrammaticPhase = status === "pending_merge";
  const isAgentPhase = status === "merging";
  const isWaitingOnPr = status === "waiting_on_pr";
  const historicalOutcome = isHistorical
    ? task.internalStatus === "merged" || task.internalStatus === "merge_conflict"
      ? task.internalStatus
      : null
    : null;
  const historicalMode = isHistorical
    ? isProgrammaticPhase
      ? "attempted"
      : isWaitingOnPr
      ? "waiting_on_pr"
      : "resolving"
    : undefined;

  // Detect validation recovery mode from task metadata
  const isValidationRecovery = useMemo(() => {
    if (!task.metadata) return false;
    try {
      const parsed = typeof task.metadata === "string" ? JSON.parse(task.metadata) : task.metadata;
      return parsed?.validation_recovery === true;
    } catch {
      return false;
    }
  }, [task.metadata]);

  // Parse merge conflict context (plan←main or source←target) from task metadata
  const mergeConflictContext = useMemo(() => {
    if (!task.metadata) return null;
    try {
      const parsed = typeof task.metadata === "string" ? JSON.parse(task.metadata) : task.metadata;
      const fallback = task.taskBranch ?? "task branch";
      if (parsed?.pr_branch_update_conflict === true) {
        return {
          type:
            parsed?.pr_branch_publication_conflict === true
              ? ("pr_branch_publication" as const)
              : ("pr_branch_update" as const),
          base: String(parsed.base_branch ?? parsed.source_branch ?? "main"),
          target: String(parsed.target_branch ?? fallback),
        };
      }
      if (parsed?.plan_update_conflict === true) {
        return { type: "plan_update" as const, base: String(parsed.base_branch ?? "main"), target: String(parsed.target_branch ?? fallback) };
      }
      if (parsed?.source_update_conflict === true) {
        return { type: "source_update" as const, source: String(parsed.source_branch ?? fallback), target: String(parsed.target_branch ?? fallback) };
      }
      return null;
    } catch {
      return null;
    }
  }, [task.metadata, task.taskBranch]);

  // Detect re-validating state from task metadata (set when fixer agent completes, cleared after re-validation)
  const isRevalidating = useMemo(() => {
    if (!task.metadata) return false;
    try {
      const parsed = typeof task.metadata === "string" ? JSON.parse(task.metadata) : task.metadata;
      return parsed?.revalidating === true;
    } catch {
      return false;
    }
  }, [task.metadata]);

  // Live validation events (only meaningful during active pending_merge)
  const liveSteps = useMergeValidationEvents(task.id);

  // High-level merge progress phases (dynamic from project analysis)
  const { phases: mergePhases, phaseList } = useMergeProgressEvents(task.id);

  // Parse conflict files from task metadata (for historical view or fallback)
  const metadataConflicts: string[] = useMemo(() => {
    if (!task.metadata) return [];
    const metadata = typeof task.metadata === "string"
      ? JSON.parse(task.metadata)
      : task.metadata;
    return Array.isArray(metadata?.conflict_files) ? metadata.conflict_files : [];
  }, [task.metadata]);

  // Live conflict detection (only active for non-historical views)
  const {
    conflicts: liveConflicts,
    isLoading: isLoadingConflicts,
    isEnabled: isConflictDetectionEnabled,
  } = useConflictDetection({
    taskId: task.id,
    internalStatus: status,
    isHistorical: isHistorical ?? false,
    hasBranch: !!task.taskBranch,
  });

  // Hybrid data source: use live conflicts for active states, metadata for historical
  const conflictFiles: string[] = isHistorical
    ? metadataConflicts
    : (isConflictDetectionEnabled && liveConflicts.length > 0 ? liveConflicts : metadataConflicts);

  const branchName = task.taskBranch ?? "task branch";

  // PR mode: fetch plan branch for PR status display
  const { data: planBranch } = usePlanBranchForTask(task.id);
  const isPrBranchUpdateConflict =
    mergeConflictContext?.type === "pr_branch_update" ||
    mergeConflictContext?.type === "pr_branch_publication";
  const isPrMode =
    (isWaitingOnPr || (isAgentPhase && !isPrBranchUpdateConflict)) &&
    planBranch?.prEligible === true &&
    planBranch?.prNumber != null;
  const isPrWait = isWaitingOnPr || isPrMode;
  const showPrReviewContext = !isHistorical && isPrWait;
  const { data: reviewHistory } = useTaskStateHistory(task.id, {
    enabled: showPrReviewContext,
  });
  const { data: stateTransitions = [] } = useTaskStateTransitions(
    showPrReviewContext ? task.id : undefined
  );

  // Resolve target branch: pipeline (most accurate) → metadata fallback
  const { data: pipelineData } = useMergePipeline(task.projectId);
  const pipelineTask = [
    ...(pipelineData?.active ?? []),
    ...(pipelineData?.waiting ?? []),
    ...(pipelineData?.needsAttention ?? []),
  ].find((t) => t.taskId === task.id);
  const resolvedTargetBranch = useMemo(() => {
    if (pipelineTask?.targetBranch) return pipelineTask.targetBranch;
    if (!task.metadata) return null;
    try {
      const parsed = typeof task.metadata === "string" ? JSON.parse(task.metadata) : task.metadata;
      return (parsed?.target_branch as string | undefined) ?? null;
    } catch {
      return null;
    }
  }, [pipelineTask?.targetBranch, task.metadata]);

  const statusLabel = historicalOutcome
    ? historicalOutcome === "merged"
      ? "Merged"
      : "Conflict"
    : isPrWait
    ? "Waiting on PR"
    : isHistorical
    ? isProgrammaticPhase
      ? "Attempted"
      : isValidationRecovery
      ? "Fixing"
      : mergeConflictContext
      ? "Updating"
      : "Resolving"
    : isProgrammaticPhase
    ? "Merging"
    : isValidationRecovery
    ? "Fixing"
    : mergeConflictContext
    ? "Updating"
    : "Resolving";
  const statusIcon = historicalOutcome
    ? historicalOutcome === "merged"
      ? CheckCircle2
      : AlertTriangle
    : isPrWait
    ? GitPullRequest
    : isHistorical
    ? isProgrammaticPhase
      ? GitMerge
      : isValidationRecovery
      ? Wrench
      : mergeConflictContext
      ? RefreshCw
      : AlertTriangle
    : isProgrammaticPhase
    ? GitMerge
    : isValidationRecovery
    ? Wrench
    : mergeConflictContext
    ? RefreshCw
    : AlertTriangle;

  return (
    <>
    <TwoColumnLayout
      description={task.description}
      testId="merging-task-detail"
    >
      {/* Status Banner */}
      <StatusBanner
        icon={
          historicalOutcome
            ? historicalOutcome === "merged"
              ? CheckCircle2
              : AlertTriangle
            : isPrWait
            ? GitPullRequest
            : isHistorical
            ? isProgrammaticPhase
              ? GitMerge
              : isValidationRecovery
              ? Wrench
              : mergeConflictContext
              ? RefreshCw
              : AlertTriangle
            : isProgrammaticPhase
            ? GitMerge
            : isValidationRecovery
            ? Wrench
            : mergeConflictContext
            ? RefreshCw
            : Bot
        }
        title={
          historicalOutcome
            ? historicalOutcome === "merged"
              ? "Merge Completed"
              : "Merge Conflict"
            : isHistorical
            ? isProgrammaticPhase
              ? "Merge Attempted"
              : isValidationRecovery
              ? "Fixing Validation Errors"
              : mergeConflictContext?.type === "pr_branch_publication"
              ? "Publishing PR Branch"
              : mergeConflictContext?.type === "pr_branch_update"
              ? "Updating PR Branch"
              : mergeConflictContext?.type === "plan_update"
              ? "Updating Plan Branch"
              : mergeConflictContext?.type === "source_update"
              ? "Updating Task Branch"
              : "Resolving Conflicts"
            : isProgrammaticPhase
            ? "Merging Changes..."
            : isPrWait
            ? "Waiting on Pull Request"
            : isValidationRecovery
            ? "Fixing Validation Errors..."
            : mergeConflictContext?.type === "pr_branch_publication"
            ? "Publishing PR Branch"
            : mergeConflictContext?.type === "pr_branch_update"
            ? "Updating PR Branch"
            : mergeConflictContext?.type === "plan_update"
            ? "Updating Plan Branch"
            : mergeConflictContext?.type === "source_update"
            ? "Updating Task Branch"
            : "Resolving Merge Conflicts"
        }
        subtitle={
          historicalOutcome
            ? historicalOutcome === "merged"
              ? "Final outcome: merged into base branch"
              : "Final outcome: manual resolution required"
            : isHistorical
            ? isProgrammaticPhase
              ? "Merge attempt captured in history"
              : isValidationRecovery
              ? "Agent was fixing build errors"
              : mergeConflictContext?.type === "pr_branch_publication"
              ? "Merger agent was reconciling the remote PR branch before publication"
              : mergeConflictContext?.type === "pr_branch_update"
              ? "Merger agent was updating the PR branch from its base branch"
              : mergeConflictContext?.type === "plan_update"
              ? "Merging latest changes from main into plan branch"
              : mergeConflictContext?.type === "source_update"
              ? "Merging latest changes from plan into task branch"
              : "Agent was resolving conflicts"
            : isProgrammaticPhase
            ? "Attempting to merge..."
            : isPrWait
            ? planBranch?.prNumber
              ? `Review and merge PR #${planBranch.prNumber} in GitHub. RalphX will finish this plan after GitHub reports it merged.`
              : "Review and merge the GitHub PR. RalphX will finish this plan after GitHub reports it merged."
            : isValidationRecovery
            ? "AI agent is fixing build errors"
            : mergeConflictContext?.type === "pr_branch_publication"
            ? planBranch?.prNumber
              ? `A merger agent is incorporating remote updates into PR #${planBranch.prNumber} so publication can finish.`
              : `A merger agent is incorporating remote PR branch updates from ${mergeConflictContext.base} so publication can finish.`
            : mergeConflictContext?.type === "pr_branch_update"
            ? planBranch?.prNumber
              ? `A merger agent is updating PR #${planBranch.prNumber} with the latest changes from ${mergeConflictContext.base} so GitHub review can continue.`
              : `A merger agent is updating the PR branch with the latest changes from ${mergeConflictContext.base} so GitHub review can continue.`
            : mergeConflictContext?.type === "plan_update"
            ? "Merging latest changes from main into plan branch"
            : mergeConflictContext?.type === "source_update"
            ? "Merging latest changes from plan into task branch"
            : "AI agent is resolving conflicts"
        }
        variant={
          historicalOutcome
            ? historicalOutcome === "merged"
              ? "success"
              : "warning"
            : isHistorical
            ? isProgrammaticPhase
              ? "info"
              : isValidationRecovery
              ? "accent"
              : mergeConflictContext
              ? "info"
              : "warning"
            : isProgrammaticPhase
            ? "accent"
            : isPrWait
            ? "info"
            : isValidationRecovery
            ? "accent"
            : mergeConflictContext
            ? "info"
            : "warning"
        }
        animated={!isHistorical}
        badge={
          <StatusPill
            icon={statusIcon}
            label={statusLabel}
            variant={
              historicalOutcome
                ? historicalOutcome === "merged"
                  ? "success"
                  : "warning"
                : isHistorical
                ? isProgrammaticPhase
                  ? "info"
                  : isValidationRecovery
                  ? "accent"
                  : mergeConflictContext
                  ? "info"
                  : "warning"
                : isProgrammaticPhase
                ? "accent"
                : isPrWait
                ? "info"
                : isValidationRecovery
                ? "accent"
                : mergeConflictContext
                ? "info"
                : "warning"
            }
            animated={!isHistorical}
            size="md"
          />
        }
      />

      {isPlanMerge && !detailContext && <PlanMergeContextSection taskId={task.id} />}

      {/* PR Mode: show PR status when polling */}
      {isPrWait && planBranch && planBranch.prNumber != null && (
        <section data-testid="pr-mode-section">
          <SectionTitle>Pull Request</SectionTitle>
          <PrModeCard
            isHistorical={isHistorical ?? false}
            planBranch={{
              prNumber: planBranch.prNumber!,
              prUrl: planBranch.prUrl,
              prStatus: planBranch.prStatus,
              prPollingActive: planBranch.prPollingActive,
            }}
          />
        </section>
      )}

      {showPrReviewContext && (
        <ChangeReviewSection
          taskId={task.id}
          history={reviewHistory}
          stateTransitions={stateTransitions}
          context="plan_merge"
        />
      )}

      {/* Merge Progress — high-level steps for historical, agent, and validation recovery (fixing) modes */}
      {(isHistorical || (!isPrWait && isAgentPhase) || (isValidationRecovery && !isRevalidating)) && (
        <section data-testid="merge-progress-section">
          <SectionTitle>{isHistorical ? "Process Details" : "Merge Progress"}</SectionTitle>
          <MergeProgressSteps
            isProgrammaticPhase={isProgrammaticPhase}
            isHistorical={isHistorical}
            historicalMode={historicalMode}
            isValidationRecovery={isValidationRecovery}
            isValidating={liveSteps.length > 0}
            isRevalidating={isRevalidating}
          />
        </section>
      )}

      {/* Dynamic phase timeline — live programmatic merge and revalidation only */}
      {!isHistorical && (isProgrammaticPhase || isRevalidating) && (
        mergePhases.length > 0 ? (
          <MergePhaseTimeline phases={mergePhases} phaseList={phaseList} />
        ) : (
          <section data-testid="merge-resuming-section">
            <SectionTitle>Merge Progress</SectionTitle>
            <DetailCard>
              <div className="flex items-center gap-2.5 py-1.5">
                <Loader2 className="w-4 h-4 animate-spin" style={{ color: "var(--status-info)" }} />
                <span className="text-[0.8125rem] text-text-primary/50">
                  Waiting for merge progress...
                </span>
              </div>
            </DetailCard>
          </section>
        )
      )}

      {/* Validation Progress — live events in live mode, metadata fallback in historical mode */}
      <ValidationProgress
        taskId={task.id}
        metadata={isHistorical ? task.metadata : null}
        liveSteps={isHistorical ? undefined : liveSteps}
      />

      {/* Conflict Files (only for agent phase, non-recovery) */}
      {isAgentPhase && !isPrWait && !isValidationRecovery && (conflictFiles.length > 0 || (isConflictDetectionEnabled && isLoadingConflicts)) && (
        <section data-testid="conflict-files-section">
          <SectionTitle>
            Conflict Files ({conflictFiles.length})
            {isConflictDetectionEnabled && isLoadingConflicts && (
              <Loader2 className="inline-block w-3.5 h-3.5 ml-2 animate-spin text-text-primary/40" />
            )}
          </SectionTitle>
          <DetailCard variant="warning">
            {isConflictDetectionEnabled && isLoadingConflicts && conflictFiles.length === 0 ? (
              <div className="flex items-center gap-2 py-2">
                <Loader2 className="w-4 h-4 animate-spin" style={{ color: "var(--status-warning)" }} />
                <span className="text-[0.8125rem] text-text-primary/50">Detecting conflicts...</span>
              </div>
            ) : (
              <ConflictFilesList files={conflictFiles} taskId={task.id} />
            )}
          </DetailCard>
        </section>
      )}

      {/* Actions — only for active (non-historical) agent-assisted merges */}
      {!isHistorical && isAgentPhase && !isPrWait && (
        <section data-testid="merging-actions-section">
          <SectionTitle>Actions</SectionTitle>
          <DetailCard>
            <div className="flex items-center gap-2">
              <button
                type="button"
                data-testid="stop-merge-action"
                onClick={handleStop}
                disabled={stopMutation.isPending}
                className="flex items-center gap-1.5 px-3 py-1.5 rounded-md text-[0.75rem] font-medium transition-colors cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed"
                style={{
                  backgroundColor: statusTint("error", 15),
                  color: "var(--status-error)",
                }}
              >
                <Square className="w-3.5 h-3.5" />
                Stop Merge
              </button>
            </div>
            {actionError && (
              <p className="mt-2 text-[0.75rem]" style={{ color: "var(--status-error)" }}>
                {actionError}
              </p>
            )}
          </DetailCard>
        </section>
      )}

      {/* Branch Info */}
      <section data-testid="branch-info-section">
        <SectionTitle muted>Branch</SectionTitle>
        {isPrWait && planBranch ? (
          <BranchFlow source={planBranch.branchName} target={planBranch.sourceBranch} size="sm" />
        ) : mergeConflictContext?.type === "pr_branch_update" ||
          mergeConflictContext?.type === "pr_branch_publication" ? (
          <BranchFlow source={mergeConflictContext.base} target={mergeConflictContext.target} size="sm" />
        ) : mergeConflictContext?.type === "plan_update" ? (
          <BranchFlow source={mergeConflictContext.base} target={mergeConflictContext.target} size="sm" />
        ) : mergeConflictContext?.type === "source_update" ? (
          <BranchFlow source={mergeConflictContext.source} target={mergeConflictContext.target} size="sm" />
        ) : resolvedTargetBranch ? (
          <BranchFlow source={branchName} target={resolvedTargetBranch} size="sm" />
        ) : (
          <BranchBadge branch={branchName} variant="muted" size="sm" />
        )}
      </section>
    </TwoColumnLayout>
    {!isHistorical && isAgentPhase && !isPrWait && <ConfirmationDialog {...confirmationDialogProps} />}
    </>
  );
}
