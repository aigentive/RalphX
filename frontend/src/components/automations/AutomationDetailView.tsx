import { type ReactNode, useCallback, useMemo } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  ArrowLeft,
  Copy,
  ExternalLink,
  GitPullRequest,
  MoreHorizontal,
  Pause,
  Pencil,
  Play,
  PlayCircle,
  RotateCcw,
  SkipForward,
  Square,
  Trash2,
} from "lucide-react";
import { toast } from "sonner";

import {
  automationsApi,
  type Automation,
  type AutomationPipelineProgress,
  type AutomationRun,
  type AutomationUsage,
} from "@/api/automations";
import { useAfterPaintMounted } from "@/components/agents/agentDeferredFrame";
import {
  AUTOMATION_CANCEL_CONFIRMATION_DESCRIPTION,
  CANCELLED_RUN_RESTART_DESCRIPTION,
  describeAutomationDeleteConsequences,
  getAutomationRunView,
  getAutomationJudgeRecovery,
  isAutomationDeletable,
  isIdleAfterCancelledRun,
  isOpenAutomationRun,
  latestRun,
} from "@/components/automations/automationStage";
import {
  AUTOMATION_PHASES_LABEL,
  AUTOMATION_STATUS_LABELS,
  findInProgressAutomationGoalItem,
  parseAutomationGoalItems,
} from "@/components/automations/automationGoalItems";
import { AutomationPhaseProgress } from "@/components/automations/AutomationPhases";
import type { AutomationRunOpenTarget } from "@/components/automations/automationRunNavigation";
import { AutomationSpecView } from "@/components/automations/AutomationSpecView";
import { AutomationDetailsTabs } from "@/components/automations/AutomationDetailsTabs";
import { RunTimelineItem } from "@/components/automations/AutomationRunTimelineItem";
import { Button, type ButtonProps } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Skeleton } from "@/components/ui/skeleton";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { useConfirmation } from "@/hooks/useConfirmation";
import {
  evictDeletedAutomation,
  invalidateAutomationQueries,
  useAutomationDetail,
} from "@/hooks/useAutomations";
import { formatDate, numberField, parseRecord, stringField } from "./automationDetailFormat";
import { ExpandableText, Pill } from "./automationDetailShared";

interface AutomationDetailViewProps {
  automationId: string;
  projectId: string | null;
  projectName?: string | null;
  onBack: () => void;
  onOpenRunConversation?: (projectId: string, conversationId: string) => void;
  onOpenAutomationRun?: (target: AutomationRunOpenTarget) => void;
}


function formatNumber(value: number): string {
  return new Intl.NumberFormat().format(value);
}

function formatEstimatedUsd(value: AutomationUsage["estimatedUsd"]): string {
  if (value === null) {
    return "Not recorded";
  }
  return new Intl.NumberFormat(undefined, {
    style: "currency",
    currency: "USD",
    maximumFractionDigits: 4,
  }).format(value);
}

function formatBase(automation: Automation): string {
  return (automation.baseDisplayName ?? automation.baseRef) || automation.baseRefKind;
}

function formatMode(automation: Automation): string {
  const effort = automation.logicalEffort ? `/${automation.logicalEffort}` : "";
  return `${automation.runMode} · ${automation.providerHarness}/${automation.modelId}${effort}`;
}

function sortedNewestRuns(runs: AutomationRun[]): AutomationRun[] {
  return [...runs].sort((a, b) => b.runIndex - a.runIndex);
}

function isSignalTerminalUnjudged(run: AutomationRun | null): run is AutomationRun {
  return Boolean(
    run
      && ["completed", "merged", "pr_closed", "agent_failed"].includes(run.status)
      && run.judgeState === "none",
  );
}

function isAutomationTerminal(status: Automation["status"]): boolean {
  return status === "completed" || status === "stopped";
}

function approvalErrorMessage(error: unknown): string {
  if (error instanceof Error && error.message.trim()) {
    return error.message;
  }
  if (typeof error === "string" && error.trim()) {
    return error;
  }
  return "Failed to approve automation";
}

function TooltipIconButton({
  label,
  tooltip,
  children,
  ...props
}: ButtonProps & { label: string; tooltip?: ReactNode }) {
  const tooltipContent = tooltip ?? label;
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          type="button"
          size="icon-sm"
          aria-label={label}
          {...(typeof tooltipContent === "string" ? { title: tooltipContent } : {})}
          {...props}
        >
          {children}
        </Button>
      </TooltipTrigger>
      <TooltipContent>{tooltipContent}</TooltipContent>
    </Tooltip>
  );
}

/**
 * Live "what's happening now" chip shown in the header while a run is open, so
 * the user sees run progress (e.g. "Run 1 in progress", "Judging") without
 * scrolling to the timeline. Styling mirrors {@link Pill} with an added pulsing
 * dot; longhand paint/border props keep it WKWebView-safe.
 */
function RunStatusChip({
  label,
  testId = "automation-run-status-chip",
}: {
  label: string;
  testId?: string;
}) {
  return (
    <span
      className="inline-flex w-fit items-center gap-1.5 rounded-full px-2 py-0.5 text-xs font-semibold text-[var(--accent-primary)]"
      style={{
        backgroundColor: "var(--bg-hover)",
        borderColor: "var(--accent-primary)",
        borderStyle: "solid",
        borderWidth: "1px",
      }}
      data-testid={testId}
    >
      <span
        className="h-1.5 w-1.5 shrink-0 animate-pulse rounded-full"
        style={{ backgroundColor: "var(--accent-primary)" }}
        aria-hidden="true"
      />
      {label}
    </span>
  );
}

function Section({
  title,
  children,
  testId,
}: {
  title: string;
  children: ReactNode;
  testId?: string;
}) {
  return (
    <section
      className="rounded-md p-4"
      style={{
        backgroundColor: "var(--bg-surface)",
        borderColor: "var(--border-default)",
        borderStyle: "solid",
        borderWidth: "1px",
      }}
      {...(testId ? { "data-testid": testId } : {})}
    >
      <h2 className="text-sm font-semibold" style={{ color: "var(--text-primary)" }}>
        {title}
      </h2>
      <div className="mt-3">{children}</div>
    </section>
  );
}

function ConfigGroup({
  title,
  items,
  testId,
}: {
  title: string;
  items: Array<[string, ReactNode]>;
  testId: string;
}) {
  return (
    <div data-testid={testId}>
      <div
        className="mb-2 text-xs font-semibold uppercase tracking-normal"
        style={{ color: "var(--text-muted)" }}
      >
        {title}
      </div>
      <KeyValueList items={items} />
    </div>
  );
}

function KeyValueList({ items }: { items: Array<[string, ReactNode]> }) {
  return (
    <dl className="grid grid-cols-1 gap-3 sm:grid-cols-2">
      {items.map(([label, value]) => (
        <div key={label} className="min-w-0">
          <dt className="text-xs font-medium uppercase tracking-normal" style={{ color: "var(--text-muted)" }}>
            {label}
          </dt>
          <dd className="mt-1 min-w-0 text-sm" style={{ color: "var(--text-secondary)" }}>
            {typeof value === "string" || typeof value === "number" ? (
              <span className="block truncate">{value}</span>
            ) : (
              value
            )}
          </dd>
        </div>
      ))}
    </dl>
  );
}

function PipelineProgress({ pipeline }: { pipeline: AutomationPipelineProgress }) {
  return (
    <div
      className="mt-4 rounded-md p-3"
      style={{
        backgroundColor: "var(--bg-hover)",
        borderColor: "var(--border-default)",
        borderStyle: "solid",
        borderWidth: "1px",
      }}
      data-testid="automation-pipeline-progress"
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div>
          <div className="text-xs font-semibold uppercase tracking-normal" style={{ color: "var(--text-muted)" }}>
            Task pipeline
          </div>
          <div className="mt-1 text-sm font-medium" style={{ color: "var(--text-primary)" }}>
            {pipeline.taskMerged} / {pipeline.taskTotal} merged
          </div>
        </div>
        <Pill label={pipeline.status} status={pipeline.status} />
      </div>
      <div className="mt-3 h-1.5 overflow-hidden rounded-full" style={{ backgroundColor: "var(--border-default)" }}>
        <div
          className="h-full rounded-full"
          style={{
            backgroundColor: "var(--accent-primary)",
            width: `${pipeline.taskTotal === 0 ? 0 : Math.round((pipeline.taskMerged / pipeline.taskTotal) * 100)}%`,
          }}
        />
      </div>
      <div className="mt-3 space-y-2">
        {pipeline.tasks.map((task) => (
          <div key={task.id} className="flex min-w-0 items-center justify-between gap-3 text-xs">
            <span className="min-w-0 truncate" style={{ color: "var(--text-secondary)" }}>
              {task.title}
            </span>
            <span className="shrink-0" style={{ color: "var(--text-muted)" }}>
              {task.blockedBy.length === 0
                ? task.status
                : `${task.blockedBy.length} ${task.blockedBy.length === 1 ? "dependency" : "dependencies"}`}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}

function BranchConfigValue({ automation }: { automation: Automation }) {
  const branchRef = automation.baseRef.trim();
  if (!branchRef) {
    return <span className="block truncate">Not recorded</span>;
  }
  const displayName = automation.baseDisplayName?.trim();
  const showDisplayName = Boolean(displayName && displayName !== branchRef);
  const copyBranch = async () => {
    try {
      if (!navigator.clipboard) {
        throw new Error("clipboard unavailable");
      }
      await navigator.clipboard.writeText(branchRef);
      toast.success("Branch copied");
    } catch {
      toast.error("Failed to copy branch");
    }
  };

  return (
    <span className="inline-flex max-w-full items-center gap-1.5 align-middle">
      <span className="min-w-0 truncate">
        {showDisplayName ? (
          <span className="mr-1" style={{ color: "var(--text-muted)" }}>
            {displayName}
          </span>
        ) : null}
        <code className="font-mono text-[0.8125rem]" data-testid="automation-branch-value">
          {branchRef}
        </code>
      </span>
      <Tooltip>
        <TooltipTrigger asChild>
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            aria-label="Copy branch"
            className="h-6 w-6 shrink-0"
            onClick={() => void copyBranch()}
            data-testid="automation-branch-copy"
          >
            <Copy className="h-3.5 w-3.5" aria-hidden="true" />
          </Button>
        </TooltipTrigger>
        <TooltipContent>Copy branch</TooltipContent>
      </Tooltip>
    </span>
  );
}

function SourcePrInput({ automation }: { automation: Automation }) {
  const sourcePr = parseRecord(automation.baseSourcePullRequestJson);
  const number = numberField(sourcePr, "number");
  const title = stringField(sourcePr, "title");
  const url = stringField(sourcePr, "url");

  if (!sourcePr) {
    return (
      <p className="text-sm" style={{ color: "var(--text-muted)" }}>
        No setup input references are attached to this automation record.
      </p>
    );
  }

  return (
    <div className="flex flex-wrap items-center gap-2 text-sm" style={{ color: "var(--text-secondary)" }}>
      <GitPullRequest className="h-4 w-4" aria-hidden="true" />
      <span>{number ? `PR #${number}` : "Source PR"}</span>
      {title && <span className="truncate">{title}</span>}
      {url && (
        <a
          href={url}
          target="_blank"
          rel="noreferrer"
          className="inline-flex items-center gap-1 text-[var(--accent-primary)]"
        >
          Open <ExternalLink className="h-3 w-3" aria-hidden="true" />
        </a>
      )}
    </div>
  );
}

function GoalItems({ value }: { value: string | null }) {
  const parsed = useMemo(
    () => parseAutomationGoalItems(value, { limit: 6 }),
    [value],
  );

  if (parsed.length === 0) {
    return null;
  }

  return (
    <div className="mt-4 space-y-2">
      <div className="text-xs font-medium uppercase tracking-normal" style={{ color: "var(--text-muted)" }}>
        {AUTOMATION_PHASES_LABEL}
      </div>
      <AutomationPhaseProgress value={value} limit={6} />
    </div>
  );
}

function DetailLoading({ onBack }: { onBack: () => void }) {
  return (
    <div
      className="flex h-full min-h-0 flex-col"
      style={{ backgroundColor: "var(--app-content-bg)" }}
      data-testid="automation-detail-loading"
    >
      <div
        className="flex items-center justify-between border-b px-6 py-5"
        style={{
          backgroundColor: "var(--app-content-bg)",
          borderBottomColor: "var(--border-default)",
          borderBottomStyle: "solid",
          borderBottomWidth: "1px",
        }}
      >
        <Button type="button" variant="ghost" onClick={onBack} className="gap-2">
          <ArrowLeft className="h-4 w-4" />
          Back
        </Button>
        <Skeleton className="h-8 w-40" />
      </div>
      <div className="grid gap-4 p-6 lg:grid-cols-[minmax(0,0.8fr)_minmax(0,1.2fr)]">
        <Skeleton className="h-72 rounded-md" />
        <Skeleton className="h-96 rounded-md" />
      </div>
    </div>
  );
}

export function AutomationDetailView({
  automationId,
  projectId,
  projectName,
  onBack,
  onOpenRunConversation,
  onOpenAutomationRun,
}: AutomationDetailViewProps) {
  const afterPaint = useAfterPaintMounted(Boolean(automationId));
  const detail = useAutomationDetail(automationId, { enabled: afterPaint });
  const queryClient = useQueryClient();
  const { confirm, confirmationDialogProps, ConfirmationDialog } = useConfirmation();
  const invalidate = useCallback(() => {
    invalidateAutomationQueries(queryClient, automationId);
  }, [automationId, queryClient]);

  const pauseMutation = useMutation({
    mutationFn: () => automationsApi.pause({
      id: automationId,
      reasonCode: "user",
      reasonDetail: "Paused from Automations detail",
    }),
    onSuccess: () => {
      invalidate();
      toast.success("Automation paused");
    },
    onError: () => toast.error("Failed to pause automation"),
  });
  const resumeMutation = useMutation({
    mutationFn: () => automationsApi.resume(automationId),
    onSuccess: () => {
      invalidate();
      toast.success("Automation resumed");
    },
    onError: () => toast.error("Failed to resume automation"),
  });
  const finalizeMutation = useMutation({
    mutationFn: () => automationsApi.finalize(automationId),
    onSuccess: () => {
      invalidate();
      toast.success("Automation spec approved");
    },
    onError: (error) => toast.error(approvalErrorMessage(error)),
  });
  const stopMutation = useMutation({
    mutationFn: () => automationsApi.stop(automationId),
    onSuccess: () => {
      invalidate();
      toast.success("Automation cancelled");
    },
    onError: () => toast.error("Failed to cancel automation"),
  });
  const restartMutation = useMutation({
    mutationFn: () => automationsApi.restart(automationId),
    onSuccess: (outcome) => {
      invalidate();
      if (outcome.scheduled) {
        toast.success("Automation restarted with a new run");
      } else {
        toast.info(outcome.reason ?? "Automation was not restarted");
      }
    },
    onError: () => toast.error("Failed to restart automation"),
  });
  const retryJudgeMutation = useMutation({
    mutationFn: () => automationsApi.retryJudge(automationId),
    onSuccess: (outcome) => {
      invalidate();
      if (outcome.scheduled) {
        toast.success("Terminal judge retry scheduled");
      } else {
        toast.info(outcome.reason ?? "Terminal judge was not retried");
      }
    },
    onError: () => toast.error("Failed to retry terminal judge"),
  });
  const retryPlanJudgeMutation = useMutation({
    mutationFn: () => automationsApi.retryPlanJudge(automationId),
    onSuccess: (outcome) => {
      invalidate();
      if (outcome.scheduled) {
        toast.success("Plan judge retry scheduled");
      } else {
        toast.info(outcome.reason ?? "Plan judge was not retried");
      }
    },
    onError: () => toast.error("Failed to retry plan judge"),
  });
  const runNowMutation = useMutation({
    mutationFn: () => automationsApi.triggerRunNow(automationId),
    onSuccess: (outcome) => {
      invalidate();
      if (outcome.scheduled) {
        toast.success("Automation run scheduled");
      } else {
        toast.info(outcome.reason ?? "Run now did not schedule work");
      }
    },
    onError: () => toast.error("Failed to run automation"),
  });
  const skipJudgeMutation = useMutation({
    mutationFn: (runId: string) => automationsApi.skipJudge({ id: automationId, runId }),
    onSuccess: (outcome) => {
      invalidate();
      if (outcome.scheduled) {
        toast.success("Judge skipped");
      } else {
        toast.info(outcome.reason ?? "Judge was not skipped");
      }
    },
    onError: () => toast.error("Failed to skip judge"),
  });
  const deleteMutation = useMutation({
    mutationFn: () => automationsApi.delete(automationId),
    onSuccess: () => {
      evictDeletedAutomation(queryClient, automationId);
      toast.success("Automation deleted");
      onBack();
    },
    onError: () => toast.error("Failed to delete automation"),
  });
  const goalItemsJson = detail.data?.automation.goalItemsJson ?? null;
  const activeGoalItem = useMemo(
    () => findInProgressAutomationGoalItem(goalItemsJson),
    [goalItemsJson],
  );

  if (!afterPaint || detail.isLoading) {
    return <DetailLoading onBack={onBack} />;
  }
  if (detail.isError || !detail.data) {
    return (
      <div
        className="flex h-full min-h-0 flex-col"
        style={{ backgroundColor: "var(--app-content-bg)" }}
      >
        <div
          className="border-b px-6 py-5"
          style={{
            borderBottomColor: "var(--border-default)",
            borderBottomStyle: "solid",
            borderBottomWidth: "1px",
          }}
        >
          <Button type="button" variant="ghost" onClick={onBack} className="gap-2">
            <ArrowLeft className="h-4 w-4" />
            Back
          </Button>
        </div>
        <div className="p-6 text-sm" style={{ color: "var(--status-error)" }}>
          Could not load automation.
        </div>
      </div>
    );
  }

  const { automation, runs } = detail.data;
  const { usage } = detail.data;
  const newestRuns = sortedNewestRuns(runs);
  const latest = latestRun(runs);
  const idleAfterCancelledRun = isIdleAfterCancelledRun(automation, latest);
  const judgeRecovery = getAutomationJudgeRecovery(automation, latest);
  const skipJudgeRun = isSignalTerminalUnjudged(latest) ? latest : null;
  // A run is only "in the way" of scheduling when the automation is actively
  // driving it. While paused, "Run now" is an explicit resume-and-override the
  // user is allowed to trigger even with a run still open, so don't block it.
  const openRun = isOpenAutomationRun(latest) ? latest : null;
  const activeRun = automation.status === "active" ? openRun : null;
  const activeRunView = activeRun ? getAutomationRunView(automation, activeRun) : null;
  const liveStageLabel = activeRunView?.stageLabel ?? null;
  const runNowBlockedReason = activeRun
    ? `${liveStageLabel} — wait for it to finish before running again`
    : automation.status === "draft"
      ? "Approve the automation before running it"
      : isAutomationTerminal(automation.status)
        ? "This automation is no longer running"
        : null;
  const actionPending = pauseMutation.isPending
    || resumeMutation.isPending
    || finalizeMutation.isPending
    || stopMutation.isPending
    || restartMutation.isPending
    || retryJudgeMutation.isPending
    || retryPlanJudgeMutation.isPending
    || runNowMutation.isPending
    || skipJudgeMutation.isPending
    || deleteMutation.isPending;

  const handleRunNow = async () => {
    if (automation.status === "paused") {
      const confirmed = await confirm({
        title: "Resume and run now?",
        description: "Run now is an explicit override that resumes the automation before scheduling eligible work.",
        confirmText: "Resume and run",
      });
      if (!confirmed) {
        return;
      }
    }
    runNowMutation.mutate();
  };

  const handleStop = async () => {
    const confirmed = await confirm({
      title: "Cancel automation?",
      description: AUTOMATION_CANCEL_CONFIRMATION_DESCRIPTION,
      confirmText: "Cancel automation",
      pendingText: "Cancelling...",
      variant: "destructive",
    });
    if (confirmed) {
      stopMutation.mutate();
    }
  };

  const handleDelete = async () => {
    const confirmed = await confirm({
      title: "Delete automation?",
      description: describeAutomationDeleteConsequences(automation, runs),
      confirmText: "Delete",
      pendingText: "Deleting...",
      variant: "destructive",
    });
    if (confirmed) {
      deleteMutation.mutate();
    }
  };

  const handleEdit = () => {
    if (projectId && automation.setupConversationId) {
      onOpenRunConversation?.(projectId, automation.setupConversationId);
    }
  };

  return (
    <div
      className="flex h-full min-h-0 flex-col"
      style={{ backgroundColor: "var(--app-content-bg)" }}
      data-testid="automation-detail-view"
    >
      <div
        className="flex flex-wrap items-center justify-between gap-3 border-b px-6 py-5"
        style={{
          backgroundColor: "var(--app-content-bg)",
          borderBottomColor: "var(--border-default)",
          borderBottomStyle: "solid",
          borderBottomWidth: "1px",
        }}
      >
        <div className="flex min-w-0 items-center gap-3">
          <Button type="button" variant="ghost" onClick={onBack} className="gap-2">
            <ArrowLeft className="h-4 w-4" />
            Back
          </Button>
          <div className="min-w-0">
            <div className="flex min-w-0 flex-wrap items-center gap-2">
              <h1 className="truncate text-xl font-semibold" style={{ color: "var(--text-primary)" }}>
                {automation.name}
              </h1>
              <Pill label={AUTOMATION_STATUS_LABELS[automation.status]} status={automation.status} />
              {liveStageLabel && <RunStatusChip label={liveStageLabel} />}
            </div>
            <div className="mt-1 text-xs" style={{ color: "var(--text-muted)" }}>
              {projectName ?? projectId ?? automation.projectId}
            </div>
          </div>
        </div>
        <div className="flex items-center gap-2">
          {automation.status === "draft" && (
            <Button
              type="button"
              variant="outline"
              className="gap-2"
              disabled={actionPending}
              onClick={() => finalizeMutation.mutate()}
            >
              <PlayCircle className="h-4 w-4" />
              Approve
            </Button>
          )}
          {automation.status === "stopped" ? (
            <Button
              type="button"
              variant="outline"
              className="gap-2"
              disabled={actionPending}
              onClick={() => restartMutation.mutate()}
            >
              <RotateCcw className="h-4 w-4" aria-hidden="true" />
              Restart automation
            </Button>
          ) : null}
          {automation.status === "paused" ? (
            <TooltipIconButton
              label="Resume automation"
              variant="outline"
              disabled={actionPending}
              onClick={() => resumeMutation.mutate()}
            >
              <Play className="h-4 w-4" />
            </TooltipIconButton>
          ) : (
            <TooltipIconButton
              label="Pause automation"
              variant="outline"
              disabled={actionPending || automation.status !== "active"}
              onClick={() => pauseMutation.mutate()}
            >
              <Pause className="h-4 w-4" />
            </TooltipIconButton>
          )}
          <TooltipIconButton
            label="Run now"
            {...(runNowBlockedReason ? { tooltip: runNowBlockedReason } : {})}
            variant="outline"
            disabled={actionPending || runNowBlockedReason !== null}
            onClick={() => void handleRunNow()}
          >
            <PlayCircle className="h-4 w-4" />
          </TooltipIconButton>
          <TooltipIconButton
            label="Cancel automation"
            variant="outline"
            disabled={actionPending || isAutomationTerminal(automation.status)}
            onClick={() => void handleStop()}
          >
            <Square className="h-4 w-4" />
          </TooltipIconButton>
          <DropdownMenu>
            <Tooltip>
              <TooltipTrigger asChild>
                <DropdownMenuTrigger asChild>
                  <Button
                    type="button"
                    size="icon-sm"
                    variant="outline"
                    aria-label="More automation actions"
                    disabled={actionPending}
                  >
                    <MoreHorizontal className="h-4 w-4" />
                  </Button>
                </DropdownMenuTrigger>
              </TooltipTrigger>
              <TooltipContent>More automation actions</TooltipContent>
            </Tooltip>
            <DropdownMenuContent align="end">
              <DropdownMenuItem
                disabled={!skipJudgeRun}
                onSelect={(event) => {
                  event.preventDefault();
                  if (skipJudgeRun) {
                    skipJudgeMutation.mutate(skipJudgeRun.id);
                  }
                }}
              >
                <SkipForward className="h-4 w-4" />
                Skip judge
              </DropdownMenuItem>
              <DropdownMenuItem
                disabled={!automation.setupConversationId || !projectId || !onOpenRunConversation}
                onSelect={(event) => {
                  event.preventDefault();
                  handleEdit();
                }}
              >
                <Pencil className="h-4 w-4" />
                Edit
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem
                disabled={!isAutomationDeletable(automation.status)}
                className="text-[var(--status-error)]"
                onSelect={(event) => {
                  event.preventDefault();
                  void handleDelete();
                }}
              >
                <Trash2 className="h-4 w-4" />
                Delete
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-auto p-6">
        {judgeRecovery ? (
          <div
            className="mb-4 flex flex-wrap items-center justify-between gap-3 rounded-md px-3 py-2 text-sm"
            style={{
              backgroundColor: "var(--bg-surface)",
              borderColor: "var(--border-default)",
              borderStyle: "solid",
              borderWidth: "1px",
              color: "var(--text-secondary)",
            }}
            data-testid={`automation-${judgeRecovery.kind}-judge-recovery`}
          >
            <span className="min-w-0">
              <strong style={{ color: "var(--text-primary)" }}>
                {judgeRecovery.statusLabel}.
              </strong>{" "}
              {judgeRecovery.description}
            </span>
            <Button
              type="button"
              variant="secondary"
              size="sm"
              disabled={actionPending}
              onClick={() =>
                judgeRecovery.kind === "plan"
                  ? retryPlanJudgeMutation.mutate()
                  : retryJudgeMutation.mutate()
              }
            >
              {judgeRecovery.actionLabel}
            </Button>
          </div>
        ) : null}
        {idleAfterCancelledRun ? (
          <div
            className="mb-4 flex flex-wrap items-center justify-between gap-3 rounded-md px-3 py-2 text-sm"
            style={{
              backgroundColor: "var(--bg-surface)",
              borderColor: "var(--border-default)",
              borderStyle: "solid",
              borderWidth: "1px",
              color: "var(--text-secondary)",
            }}
            data-testid="automation-idle-after-cancelled"
          >
            <span className="min-w-0">
              {CANCELLED_RUN_RESTART_DESCRIPTION}
            </span>
            <Button
              type="button"
              variant="secondary"
              size="sm"
              disabled={actionPending}
              onClick={() => void handleRunNow()}
            >
              Run now
            </Button>
          </div>
        ) : null}
        <div className="grid gap-4 lg:grid-cols-[minmax(0,0.8fr)_minmax(0,1.2fr)]">
          <div className="space-y-4">
            <Section title="Goal" testId="automation-goal-card">
              <ExpandableText text={automation.goalPrompt} />
              <GoalItems value={automation.goalItemsJson} />
              {detail.data.pipeline ? <PipelineProgress pipeline={detail.data.pipeline} /> : null}
            </Section>
            <AutomationDetailsTabs
              hasSpec={Boolean(automation.specArtifactId)}
              inputCount={parseRecord(automation.baseSourcePullRequestJson) ? 1 : 0}
              spec={<AutomationSpecView specArtifactId={automation.specArtifactId} />}
              inputs={<SourcePrInput automation={automation} />}
              config={(
                <>
                  <div className="space-y-4">
                    <ConfigGroup
                      title="Execution"
                      testId="automation-config-group-execution"
                      items={[
                        ["Mode / model", formatMode(automation)] as [string, ReactNode],
                        [
                          "Setup conversation",
                          automation.setupConversationId ? (
                            <Button
                              type="button"
                              variant="link"
                              className="h-auto gap-1 p-0 text-sm"
                              disabled={!projectId || !onOpenRunConversation}
                              onClick={handleEdit}
                              data-testid="automation-setup-conversation-link"
                            >
                              Open setup conversation
                              <ExternalLink className="h-3 w-3" aria-hidden="true" />
                            </Button>
                          ) : (
                            "Not recorded"
                          ),
                        ] as [string, ReactNode],
                        ["Base", formatBase(automation)] as [string, ReactNode],
                        ...(automation.baseRef.trim()
                          ? [["Branch", <BranchConfigValue automation={automation} />] as [string, ReactNode]]
                          : []),
                        ["Chain mode", automation.chainMode] as [string, ReactNode],
                        ["Completion signal", automation.completionSignal] as [string, ReactNode],
                      ]}
                    />
                    <ConfigGroup
                      title="Limits"
                      testId="automation-config-group-limits"
                      items={[
                        ["Max runs", `${runs.length} / ${automation.maxRuns}`],
                        ["Max failures", automation.maxConsecutiveFailures],
                      ]}
                    />
                    <ConfigGroup
                      title="Usage"
                      testId="automation-config-group-usage"
                      items={[
                        ["Input tokens", formatNumber(usage.inputTokens)] as [string, ReactNode],
                        ["Output tokens", formatNumber(usage.outputTokens)] as [string, ReactNode],
                        ["Cache tokens", formatNumber(usage.cacheCreationTokens + usage.cacheReadTokens)] as [string, ReactNode],
                        ...(usage.estimatedUsd !== null
                          ? [["Estimated cost", formatEstimatedUsd(usage.estimatedUsd)] as [string, ReactNode]]
                          : []),
                      ]}
                    />
                    <p
                      className="text-xs"
                      style={{ color: "var(--text-muted)" }}
                      data-testid="automation-config-timestamps"
                    >
                      Created {formatDate(automation.createdAt)} · Updated {formatDate(automation.updatedAt)}
                    </p>
                  </div>
                  {automation.pausedReasonCode && (
                    <div className="mt-4 rounded-md p-3 text-sm" style={{
                      backgroundColor: "var(--bg-hover)",
                      color: "var(--text-secondary)",
                    }}>
                      Paused: {automation.pausedReasonCode}
                      {automation.pausedReasonDetail ? ` - ${automation.pausedReasonDetail}` : ""}
                    </div>
                  )}
                </>
              )}
            />
          </div>

          <Section title="Runs timeline" testId="automation-runs-timeline">
            {newestRuns.length === 0 ? (
              <p className="text-sm" style={{ color: "var(--text-muted)" }}>
                No runs have been created yet.
              </p>
            ) : (
              <div className="relative space-y-4 before:absolute before:bottom-0 before:left-[5px] before:top-2 before:w-px before:bg-[var(--border-default)]">
                {newestRuns.map((run) => (
                  <RunTimelineItem
                    key={run.id}
                    run={run}
                    automation={automation}
                    projectId={projectId}
                    defaultExpanded={
                      run.runIndex === latest?.runIndex || isOpenAutomationRun(run)
                    }
                    activeGoalItem={activeGoalItem}
                    {...(onOpenRunConversation ? { onOpenRunConversation } : {})}
                    {...(onOpenAutomationRun ? { onOpenAutomationRun } : {})}
                    setupConversationId={automation.setupConversationId}
                  />
                ))}
              </div>
            )}
          </Section>
        </div>
      </div>
      <ConfirmationDialog {...confirmationDialogProps} />
    </div>
  );
}
