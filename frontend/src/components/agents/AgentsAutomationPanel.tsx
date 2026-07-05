import { useCallback } from "react";
import { ExternalLink, Pause, Play, Square, Workflow } from "lucide-react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import { automationsApi, type Automation, type AutomationRun } from "@/api/automations";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { useAfterPaintMounted } from "@/components/agents/agentDeferredFrame";
import {
  invalidateAutomationQueries,
  useAutomationDetail,
  useAutomationEvents,
} from "@/hooks/useAutomations";
import { useConfirmation } from "@/hooks/useConfirmation";

interface AgentsAutomationPanelProps {
  automationId: string;
  onOpenAutomation?: (automationId: string) => void;
}

const STATUS_LABELS: Record<Automation["status"], string> = {
  draft: "Draft",
  active: "Active",
  paused: "Paused",
  completed: "Completed",
  stopped: "Stopped",
};

function latestRun(runs: AutomationRun[]): AutomationRun | null {
  return runs.reduce<AutomationRun | null>(
    (latest, run) => (!latest || run.runIndex > latest.runIndex ? run : latest),
    null,
  );
}

function formatRunSummary(run: AutomationRun | null, maxRuns: number): string {
  if (!run) {
    return `0 of ${maxRuns}`;
  }
  return `${run.runIndex} of ${maxRuns}`;
}

function formatPrState(run: AutomationRun | null): string {
  if (!run) {
    return "No PR yet";
  }
  if (!run.prNumber) {
    return run.status;
  }
  return `PR #${run.prNumber} · ${run.status}`;
}

function PanelShell() {
  return (
    <div className="space-y-4 p-5" data-testid="agents-automation-panel-loading">
      <Skeleton className="h-5 w-40" />
      <div
        className="grid gap-3 rounded-md p-4"
        style={{
          backgroundColor: "var(--bg-surface)",
          borderColor: "var(--border-default)",
          borderStyle: "solid",
          borderWidth: "1px",
        }}
      >
        <Skeleton className="h-4 w-28" />
        <Skeleton className="h-4 w-44" />
        <Skeleton className="h-4 w-36" />
      </div>
      <div className="flex gap-2">
        <Skeleton className="h-9 w-24" />
        <Skeleton className="h-9 w-24" />
      </div>
    </div>
  );
}

export function AgentsAutomationPanel({
  automationId,
  onOpenAutomation,
}: AgentsAutomationPanelProps) {
  const afterPaint = useAfterPaintMounted(Boolean(automationId));
  const detail = useAutomationDetail(automationId, { enabled: afterPaint });
  const queryClient = useQueryClient();
  const { confirm, confirmationDialogProps, ConfirmationDialog } = useConfirmation();
  useAutomationEvents(automationId);

  const invalidate = useCallback(() => {
    invalidateAutomationQueries(queryClient, automationId);
  }, [automationId, queryClient]);

  const pauseMutation = useMutation({
    mutationFn: () =>
      automationsApi.pause({
        id: automationId,
        reasonCode: "user",
        reasonDetail: "Paused from Agents automation panel",
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
  const stopMutation = useMutation({
    mutationFn: () => automationsApi.stop(automationId),
    onSuccess: () => {
      invalidate();
      toast.success("Automation stopped");
    },
    onError: () => toast.error("Failed to stop automation"),
  });

  const handleStop = async () => {
    const confirmed = await confirm({
      title: "Stop automation?",
      description: "Stopping is terminal for this automation.",
      confirmText: "Stop",
      pendingText: "Stopping...",
      variant: "destructive",
    });
    if (confirmed) {
      stopMutation.mutate();
    }
  };

  if (!afterPaint || detail.isLoading) {
    return <PanelShell />;
  }

  if (detail.isError || !detail.data) {
    return (
      <div className="p-5 text-sm" style={{ color: "var(--status-error)" }}>
        Could not load automation.
      </div>
    );
  }

  const { automation, runs } = detail.data;
  const run = latestRun(runs);
  const actionPending =
    pauseMutation.isPending || resumeMutation.isPending || stopMutation.isPending;
  const canPause = automation.status === "active";
  const canResume = automation.status === "paused";
  const canStop = automation.status !== "completed" && automation.status !== "stopped";

  return (
    <div className="space-y-4 p-5" data-testid="agents-automation-panel">
      <div className="flex items-start gap-3">
        <div
          className="grid h-9 w-9 shrink-0 place-items-center rounded-md"
          style={{ backgroundColor: "rgba(255, 107, 53, 0.14)" }}
          aria-hidden="true"
        >
          <Workflow className="h-5 w-5" style={{ color: "#ff6b35" }} />
        </div>
        <div className="min-w-0">
          <h2 className="truncate text-sm font-semibold" style={{ color: "var(--text-primary)" }}>
            {automation.name}
          </h2>
          <p className="mt-1 text-xs" style={{ color: "var(--text-muted)" }}>
            Automation-owned conversation
          </p>
        </div>
      </div>

      <div
        className="grid gap-3 rounded-md p-4 text-sm"
        style={{
          backgroundColor: "var(--bg-surface)",
          borderColor: "var(--border-default)",
          borderStyle: "solid",
          borderWidth: "1px",
        }}
      >
        <SummaryRow label="Status" value={STATUS_LABELS[automation.status]} />
        <SummaryRow label="Run" value={formatRunSummary(run, automation.maxRuns)} />
        <SummaryRow label="Current PR" value={formatPrState(run)} />
      </div>

      <div className="flex flex-wrap gap-2">
        {canPause ? (
          <Button
            type="button"
            variant="secondary"
            size="sm"
            className="gap-2"
            disabled={actionPending}
            onClick={() => pauseMutation.mutate()}
            data-testid="agents-automation-pause"
          >
            <Pause className="h-4 w-4" />
            Pause
          </Button>
        ) : null}
        {canResume ? (
          <Button
            type="button"
            variant="secondary"
            size="sm"
            className="gap-2"
            disabled={actionPending}
            onClick={() => resumeMutation.mutate()}
            data-testid="agents-automation-resume"
          >
            <Play className="h-4 w-4" />
            Resume
          </Button>
        ) : null}
        {canStop ? (
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="gap-2"
            disabled={actionPending}
            onClick={handleStop}
            data-testid="agents-automation-stop"
          >
            <Square className="h-4 w-4" />
            Stop
          </Button>
        ) : null}
        <Button
          type="button"
          size="sm"
          className="gap-2"
          disabled={!onOpenAutomation}
          onClick={() => onOpenAutomation?.(automation.id)}
          data-testid="agents-automation-open"
        >
          <ExternalLink className="h-4 w-4" />
          Open automation
        </Button>
      </div>
      <ConfirmationDialog {...confirmationDialogProps} />
    </div>
  );
}

function SummaryRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="grid grid-cols-[96px_minmax(0,1fr)] gap-3">
      <span className="text-xs font-medium" style={{ color: "var(--text-muted)" }}>
        {label}
      </span>
      <span className="min-w-0 truncate text-xs font-semibold" style={{ color: "var(--text-primary)" }}>
        {value}
      </span>
    </div>
  );
}
