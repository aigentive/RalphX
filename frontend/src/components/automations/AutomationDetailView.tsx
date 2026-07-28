import { useCallback, useMemo, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { ArrowLeft } from "lucide-react";
import { toast } from "sonner";

import { automationsApi, type Automation, type AutomationRun } from "@/api/automations";
import { useAfterPaintMounted } from "@/components/agents/agentDeferredFrame";
import {
  AUTOMATION_CANCEL_CONFIRMATION_DESCRIPTION,
  describeAutomationDeleteConsequences,
  getAutomationRunView,
  getAutomationJudgeRecovery,
  isIdleAfterCancelledRun,
  isOpenAutomationRun,
  latestRun,
} from "@/components/automations/automationStage";
import { findInProgressAutomationGoalItem } from "@/components/automations/automationGoalItems";
import { AutomationDetailHeader } from "@/components/automations/AutomationDetailHeader";
import { AutomationOverviewTab } from "@/components/automations/AutomationOverviewTab";
import { AutomationRunsTab } from "@/components/automations/AutomationRunsTab";
import { AutomationStatCards } from "@/components/automations/AutomationStatCards";
import type { AutomationRunOpenTarget } from "@/components/automations/automationRunNavigation";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useConfirmation } from "@/hooks/useConfirmation";
import {
  evictDeletedAutomation,
  invalidateAutomationQueries,
  useAutomationDetail,
} from "@/hooks/useAutomations";

interface AutomationDetailViewProps {
  automationId: string;
  projectId: string | null;
  projectName?: string | null;
  onBack: () => void;
  onOpenRunConversation?: (projectId: string, conversationId: string) => void;
  onOpenAutomationRun?: (target: AutomationRunOpenTarget) => void;
}

type AutomationDetailTab = "overview" | "runs";

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

function RunsTabLoading() {
  return (
    <div className="space-y-4" data-testid="automation-runs-tab-loading">
      <Skeleton className="h-24 rounded-md" />
      <Skeleton className="h-24 rounded-md" />
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
  // Tab selection is local presentation state; switching swaps content
  // synchronously while the heavy Runs timeline hydrates after paint.
  const [selectedTab, setSelectedTab] = useState<AutomationDetailTab>("overview");
  const runsTabMounted = useAfterPaintMounted(selectedTab === "runs");
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
  const deleteRunMutation = useMutation({
    mutationFn: (runId: string) => automationsApi.deleteRun({ id: automationId, runId }),
    onSuccess: () => {
      invalidate();
      toast.success("Run deleted");
    },
    onError: () => toast.error("Failed to delete run"),
  });
  const resumeRunMutation = useMutation({
    mutationFn: (runId: string) => automationsApi.resumeRun({ id: automationId, runId }),
    onSuccess: () => {
      invalidate();
      toast.success("Run resumed");
    },
    onError: () => toast.error("Failed to resume run"),
  });
  const handleDeleteRun = useCallback(async (run: AutomationRun) => {
    const running = run.status === "running";
    const confirmed = await confirm({
      title: running ? "Stop and delete run?" : "Delete run?",
      description: running
        ? `This stops the running agent, then permanently deletes Run ${run.runIndex}, its worktree, and branch. The conversation is archived. This cannot be undone.`
        : `This permanently deletes Run ${run.runIndex}, its worktree, and branch. The conversation is archived. This cannot be undone.`,
      confirmText: running ? "Stop & delete" : "Delete",
      pendingText: running ? "Stopping..." : "Deleting...",
      variant: "destructive",
    });
    if (confirmed) {
      deleteRunMutation.mutate(run.id);
    }
  }, [confirm, deleteRunMutation]);
  const handleResumeRun = useCallback(async (run: AutomationRun) => {
    const confirmed = await confirm({
      title: "Resume run?",
      description: `Reopens Run ${run.runIndex} in place and continues the existing agent in its worktree — prior work and conversation history are kept, nothing restarts.`,
      confirmText: "Resume",
      pendingText: "Resuming...",
      variant: "default",
    });
    if (confirmed) {
      resumeRunMutation.mutate(run.id);
    }
  }, [confirm, resumeRunMutation]);
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
    || deleteRunMutation.isPending
    || resumeRunMutation.isPending
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
      <AutomationDetailHeader
        automation={automation}
        projectLabel={projectName ?? projectId ?? automation.projectId}
        liveStageLabel={liveStageLabel}
        actionPending={actionPending}
        runNowBlockedReason={runNowBlockedReason}
        skipJudgeRunId={skipJudgeRun?.id ?? null}
        canOpenSetupConversation={Boolean(
          automation.setupConversationId && projectId && onOpenRunConversation,
        )}
        automationTerminal={isAutomationTerminal(automation.status)}
        onBack={onBack}
        onApprove={() => finalizeMutation.mutate()}
        onRestart={() => restartMutation.mutate()}
        onResume={() => resumeMutation.mutate()}
        onPause={() => pauseMutation.mutate()}
        onRunNow={() => void handleRunNow()}
        onCancelAutomation={() => void handleStop()}
        onSkipJudge={(runId) => skipJudgeMutation.mutate(runId)}
        onEdit={handleEdit}
        onDelete={() => void handleDelete()}
      />

      <div className="min-h-0 flex-1 overflow-auto p-6">
        <Tabs
          value={selectedTab}
          onValueChange={(value) => setSelectedTab(value as AutomationDetailTab)}
        >
          <AutomationStatCards automation={automation} runs={runs} />
          <div className="my-4 flex flex-wrap items-center justify-between gap-3">
            <TabsList
              className="h-9 justify-start rounded-md p-1"
              style={{
                backgroundColor: "var(--bg-surface, #1e1e23)",
                borderColor: "var(--border-subtle, #2e2e36)",
                borderStyle: "solid",
                borderWidth: "1px",
              }}
              aria-label="Automation detail sections"
            >
              <TabsTrigger
                className="rounded-sm px-3 py-1 text-xs text-[var(--text-secondary)] data-[state=active]:bg-[var(--bg-elevated)] data-[state=active]:text-[var(--text-primary)] data-[state=active]:shadow-sm"
                value="overview"
                data-testid="automation-tab-overview"
              >
                Overview
              </TabsTrigger>
              <TabsTrigger
                className="rounded-sm px-3 py-1 text-xs text-[var(--text-secondary)] data-[state=active]:bg-[var(--bg-elevated)] data-[state=active]:text-[var(--text-primary)] data-[state=active]:shadow-sm"
                value="runs"
                data-testid="automation-tab-runs"
              >
                Runs
                {runs.length > 0 ? (
                  <span
                    className="ml-1.5 rounded-full px-1.5 py-0.5 text-[0.625rem] tabular-nums"
                    style={{
                      backgroundColor: "var(--bg-hover, #2a2a31)",
                      color: "var(--text-secondary, #c7c7cc)",
                    }}
                    data-testid="automation-runs-count"
                  >
                    {runs.length}
                  </span>
                ) : null}
                {openRun ? (
                  <span
                    className="ml-1.5 h-1.5 w-1.5 shrink-0 animate-pulse rounded-full"
                    style={{ backgroundColor: "var(--accent-primary)" }}
                    aria-hidden="true"
                    data-testid="automation-tab-runs-live-dot"
                  />
                ) : null}
              </TabsTrigger>
            </TabsList>
            <div
              className="min-w-0 truncate text-xs"
              style={{ color: "var(--text-muted, #8e8e96)" }}
              data-testid="automation-detail-branch-meta"
            >
              base{" "}
              {automation.baseTargetDisplayName
                ?? automation.baseTargetRef
                ?? automation.baseDisplayName
                ?? automation.baseRef}
              {" · "}chain {automation.chainMode.replace(/_/g, " ")}
            </div>
          </div>
          <TabsContent value="overview" data-testid="automation-overview-tab">
            <AutomationOverviewTab
              automation={automation}
              runs={runs}
              usage={usage}
              pipeline={detail.data.pipeline ?? null}
              canOpenSetupConversation={Boolean(projectId && onOpenRunConversation)}
              onOpenSetupConversation={handleEdit}
            />
          </TabsContent>
          <TabsContent value="runs" data-testid="automation-runs-tab">
            {runsTabMounted ? (
              <AutomationRunsTab
                automation={automation}
                runs={runs}
                latest={latest}
                activeGoalItem={activeGoalItem}
                projectId={projectId}
                judgeRecovery={judgeRecovery}
                idleAfterCancelledRun={idleAfterCancelledRun}
                actionPending={actionPending}
                onRetryPlanJudge={() => retryPlanJudgeMutation.mutate()}
                onRetryJudge={() => retryJudgeMutation.mutate()}
                onRunNow={() => void handleRunNow()}
                onDeleteRun={handleDeleteRun}
                onResumeRun={handleResumeRun}
                {...(onOpenRunConversation ? { onOpenRunConversation } : {})}
                {...(onOpenAutomationRun ? { onOpenAutomationRun } : {})}
              />
            ) : (
              <RunsTabLoading />
            )}
          </TabsContent>
        </Tabs>
      </div>
      <ConfirmationDialog {...confirmationDialogProps} />
    </div>
  );
}
