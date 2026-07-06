import { useCallback, type ReactNode } from "react";
import { ExternalLink, Pause, Play, Square, Workflow } from "lucide-react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import { automationsApi, type Automation, type AutomationRun } from "@/api/automations";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { useAfterPaintMounted } from "@/components/agents/agentDeferredFrame";
import {
  describeAutomationStage,
  describeRunFailure,
  latestRun,
} from "@/components/automations/automationStage";
import {
  invalidateAutomationQueries,
  useAutomationDetail,
  useAutomationEvents,
} from "@/hooks/useAutomations";
import { useConfirmation } from "@/hooks/useConfirmation";
import { withAlpha } from "@/lib/theme-colors";

interface AgentsAutomationPanelProps {
  automationId: string;
  conversationTitle?: string | null;
  onOpenAutomation?: (automationId: string) => void;
}

const STATUS_LABELS: Record<Automation["status"], string> = {
  draft: "Draft",
  active: "Approved",
  paused: "Paused",
  completed: "Completed",
  stopped: "Stopped",
};
const OPEN_RUN_STATUSES = new Set<AutomationRun["status"]>([
  "pending",
  "provisioning",
  "running",
  "published",
]);

type AutomationGoalItem = {
  id: string;
  title: string;
  status: string;
};

function parseAutomationGoalItems(value: string | null): AutomationGoalItem[] {
  if (!value?.trim()) {
    return [];
  }
  try {
    const parsed = JSON.parse(value) as unknown;
    if (!Array.isArray(parsed)) {
      return [];
    }
    return parsed.flatMap((item, index) => {
      if (!item || typeof item !== "object") {
        return [];
      }
      const record = item as Record<string, unknown>;
      const title =
        typeof record.title === "string" && record.title.trim()
          ? record.title.trim()
          : typeof record.text === "string" && record.text.trim()
            ? record.text.trim()
            : `Phase ${index + 1}`;
      const id =
        typeof record.id === "string" && record.id.trim()
          ? record.id.trim()
          : `phase-${index + 1}`;
      const status =
        typeof record.status === "string" && record.status.trim()
          ? record.status.trim()
          : "pending";
      return [{ id, title, status }];
    });
  } catch {
    return [];
  }
}

function formatBase(automation: Automation): string {
  return (automation.baseDisplayName ?? automation.baseRef) || automation.baseRefKind;
}

function formatModel(automation: Automation): string {
  const effort = automation.logicalEffort ? `/${automation.logicalEffort}` : "";
  return `${automation.providerHarness}/${automation.modelId}${effort}`;
}

function automationDisplayName(
  automation: Automation,
  conversationTitle?: string | null,
): string {
  const name = automation.name.trim();
  if (name && name.toLowerCase() !== "untitled automation") {
    return name;
  }
  const title = conversationTitle?.trim();
  return title || "Automation setup";
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
  const status = OPEN_RUN_STATUSES.has(run.status) ? "Running" : run.status;
  if (!run.prNumber) {
    return status;
  }
  return `PR #${run.prNumber} · ${status}`;
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
  conversationTitle,
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
  const displayName = automationDisplayName(automation, conversationTitle);
  const run = latestRun(runs);
  const goalItems = parseAutomationGoalItems(automation.goalItemsJson);
  const stage = describeAutomationStage(automation, run);
  const failureReason = describeRunFailure(run);
  const showPausedReason =
    !failureReason && automation.status === "paused" && Boolean(automation.pausedReasonCode);
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
          style={{ backgroundColor: withAlpha("var(--accent-primary)", 14) }}
          aria-hidden="true"
        >
          <Workflow className="h-5 w-5" style={{ color: "var(--accent-primary)" }} />
        </div>
        <div className="min-w-0">
          <h2 className="truncate text-sm font-semibold" style={{ color: "var(--text-primary)" }}>
            {displayName}
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
        <SummaryRow label="Stage" value={stage} testId="agents-automation-stage" />
        <SummaryRow label="Run type" value={automation.runMode} />
        <SummaryRow label="Model" value={formatModel(automation)} />
        <SummaryRow label="Base" value={formatBase(automation)} />
        <SummaryRow label="Run" value={formatRunSummary(run, automation.maxRuns)} />
        <SummaryRow label="Current PR" value={formatPrState(run)} />
      </div>

      <DetailSection title="Goal" testId="agents-automation-goal">
        <p className="text-xs leading-5" style={{ color: "var(--text-primary)" }}>
          {automation.goalPrompt.trim() || "No goal configured yet."}
        </p>
      </DetailSection>

      <DetailSection title="Phases" testId="agents-automation-phases">
        {goalItems.length > 0 ? (
          <div className="space-y-2">
            {goalItems.map((item, index) => (
              <div
                key={`${item.id}:${index}`}
                className="grid grid-cols-[minmax(0,1fr)_auto] gap-3 text-xs"
              >
                <span
                  className="min-w-0 truncate font-medium"
                  style={{ color: "var(--text-primary)" }}
                >
                  {item.title}
                </span>
                <span style={{ color: "var(--text-muted)" }}>{item.status}</span>
              </div>
            ))}
          </div>
        ) : (
          <p className="text-xs" style={{ color: "var(--text-muted)" }}>
            No phases configured yet.
          </p>
        )}
      </DetailSection>

      {automation.setupAnalysisSummary ? (
        <DetailSection title="Setup summary" testId="agents-automation-setup-summary">
          <p className="text-xs leading-5" style={{ color: "var(--text-primary)" }}>
            {automation.setupAnalysisSummary}
          </p>
        </DetailSection>
      ) : null}

      <DetailSection title="First run" testId="agents-automation-first-run">
        <p className="text-xs leading-5" style={{ color: "var(--text-primary)" }}>
          {automation.firstRunPrompt?.trim() || "No first run prompt configured yet."}
        </p>
      </DetailSection>

      {failureReason ? (
        <div
          className="rounded-md px-3 py-2 text-xs font-medium"
          style={{
            backgroundColor: "var(--bg-surface)",
            borderColor: "var(--border-default)",
            borderStyle: "solid",
            borderWidth: "1px",
            color: "var(--status-error)",
          }}
          data-testid="agents-automation-failure"
        >
          {failureReason}
        </div>
      ) : showPausedReason ? (
        <div
          className="rounded-md px-3 py-2 text-xs"
          style={{
            backgroundColor: "var(--bg-hover)",
            color: "var(--text-secondary)",
          }}
          data-testid="agents-automation-paused"
        >
          Paused: {automation.pausedReasonCode}
          {automation.pausedReasonDetail ? ` - ${automation.pausedReasonDetail}` : ""}
        </div>
      ) : null}

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

function SummaryRow({
  label,
  value,
  testId,
}: {
  label: string;
  value: string;
  testId?: string;
}) {
  return (
    <div
      className="grid grid-cols-[96px_minmax(0,1fr)] gap-3"
      {...(testId ? { "data-testid": testId } : {})}
    >
      <span className="text-xs font-medium" style={{ color: "var(--text-muted)" }}>
        {label}
      </span>
      <span className="min-w-0 truncate text-xs font-semibold" style={{ color: "var(--text-primary)" }}>
        {value}
      </span>
    </div>
  );
}

function DetailSection({
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
      <h3
        className="mb-3 text-xs font-semibold uppercase tracking-[0.08em]"
        style={{ color: "var(--text-muted)" }}
      >
        {title}
      </h3>
      {children}
    </section>
  );
}
