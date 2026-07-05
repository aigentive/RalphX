import { memo, type ReactNode, useCallback, useMemo, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  ArrowLeft,
  ChevronDown,
  ChevronUp,
  ExternalLink,
  GitPullRequest,
  MoreHorizontal,
  Pause,
  Pencil,
  Play,
  PlayCircle,
  SkipForward,
  Square,
  Trash2,
} from "lucide-react";
import { toast } from "sonner";

import {
  automationsApi,
  type Automation,
  type AutomationRun,
  type AutomationUsage,
} from "@/api/automations";
import { useAfterPaintMounted } from "@/components/agents/agentDeferredFrame";
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
  invalidateAutomationQueries,
  useAutomationDetail,
} from "@/hooks/useAutomations";
import { cn } from "@/lib/utils";

interface AutomationDetailViewProps {
  automationId: string;
  projectId: string | null;
  projectName?: string | null;
  onBack: () => void;
  onOpenRunConversation?: (projectId: string, conversationId: string) => void;
}

type JsonRecord = Record<string, unknown>;

const AUTOMATION_STATUS_LABELS: Record<Automation["status"], string> = {
  draft: "Draft",
  active: "Active",
  paused: "Paused",
  completed: "Completed",
  stopped: "Stopped",
};

const RUN_STATUS_LABELS: Record<AutomationRun["status"], string> = {
  pending: "Pending",
  provisioning: "Provisioning",
  running: "Running",
  published: "Published",
  merged: "Merged",
  pr_closed: "PR closed",
  agent_failed: "Agent failed",
  cancelled: "Cancelled",
};

const PROMPT_AUTHOR_LABELS: Record<AutomationRun["promptAuthor"], string> = {
  setup_agent: "Setup agent",
  judge: "Judge",
  skip_judge_template: "Skip-judge template",
};

function parseRecord(value: string | null): JsonRecord | null {
  if (!value) {
    return null;
  }
  try {
    const parsed = JSON.parse(value) as unknown;
    return parsed && typeof parsed === "object" && !Array.isArray(parsed)
      ? parsed as JsonRecord
      : null;
  } catch {
    return null;
  }
}

function stringField(record: JsonRecord | null, key: string): string | null {
  const value = record?.[key];
  return typeof value === "string" && value.trim() ? value : null;
}

function numberField(record: JsonRecord | null, key: string): number | null {
  const value = record?.[key];
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function formatDate(value: string | null): string {
  if (!value) {
    return "Not recorded";
  }
  const parsed = new Date(value);
  if (Number.isNaN(parsed.getTime())) {
    return value;
  }
  return parsed.toLocaleString(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  });
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

function latestRun(runs: AutomationRun[]): AutomationRun | null {
  return runs.reduce<AutomationRun | null>(
    (latest, run) => (!latest || run.runIndex > latest.runIndex ? run : latest),
    null,
  );
}

function isSignalTerminalUnjudged(run: AutomationRun | null): run is AutomationRun {
  return Boolean(
    run
      && ["merged", "pr_closed", "agent_failed"].includes(run.status)
      && run.judgeState === "none",
  );
}

function isAutomationTerminal(status: Automation["status"]): boolean {
  return status === "completed" || status === "stopped";
}

function statusClass(status: string): string {
  if (["active", "running", "published", "merged", "completed", "done"].includes(status)) {
    return "text-[var(--status-success)]";
  }
  if (["paused", "failed", "agent_failed", "pr_closed"].includes(status)) {
    return "text-[var(--status-warning)]";
  }
  if (["stopped", "cancelled"].includes(status)) {
    return "text-[var(--status-error)]";
  }
  return "text-[var(--text-secondary)]";
}

function Pill({ label, status }: { label: string; status: string }) {
  return (
    <span
      className={cn(
        "inline-flex w-fit items-center rounded-full px-2 py-0.5 text-xs font-semibold",
        statusClass(status),
      )}
      style={{
        backgroundColor: "var(--bg-hover)",
        borderColor: "var(--border-default)",
        borderStyle: "solid",
        borderWidth: "1px",
      }}
    >
      {label}
    </span>
  );
}

function TooltipIconButton({
  label,
  children,
  ...props
}: ButtonProps & { label: string }) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          type="button"
          size="icon-sm"
          aria-label={label}
          {...props}
        >
          {children}
        </Button>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
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

function KeyValueList({ items }: { items: Array<[string, ReactNode]> }) {
  return (
    <dl className="grid grid-cols-1 gap-3 sm:grid-cols-2">
      {items.map(([label, value]) => (
        <div key={label} className="min-w-0">
          <dt className="text-xs font-medium uppercase tracking-normal" style={{ color: "var(--text-muted)" }}>
            {label}
          </dt>
          <dd className="mt-1 truncate text-sm" style={{ color: "var(--text-secondary)" }}>
            {value}
          </dd>
        </div>
      ))}
    </dl>
  );
}

function previewLines(text: string, maxLines: number): string {
  return text.split(/\r?\n/).slice(0, maxLines).join("\n");
}

function ExpandableText({
  text,
  maxLines = 10,
  emptyLabel = "Not recorded",
}: {
  text: string | null;
  maxLines?: number;
  emptyLabel?: string;
}) {
  const [expanded, setExpanded] = useState(false);
  const value = text?.trim() || "";
  if (!value) {
    return <p className="text-sm" style={{ color: "var(--text-muted)" }}>{emptyLabel}</p>;
  }
  const lines = value.split(/\r?\n/);
  const expandable = lines.length > maxLines || value.length > 900;
  const renderedText = expanded || !expandable
    ? value
    : `${previewLines(value, maxLines)}\n...`;

  return (
    <div className="space-y-2">
      <pre
        className="whitespace-pre-wrap break-words rounded-md p-3 text-xs leading-5"
        style={{
          backgroundColor: "var(--bg-hover)",
          color: "var(--text-secondary)",
        }}
      >
        {renderedText}
      </pre>
      {expandable && (
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="gap-2"
          onClick={() => setExpanded((value) => !value)}
        >
          {expanded ? <ChevronUp className="h-4 w-4" /> : <ChevronDown className="h-4 w-4" />}
          {expanded ? "Collapse" : "Expand"}
        </Button>
      )}
    </div>
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
  const parsed = useMemo(() => {
    if (!value) {
      return [];
    }
    try {
      const items = JSON.parse(value) as unknown;
      return Array.isArray(items) ? items.slice(0, 6) : [];
    } catch {
      return [];
    }
  }, [value]);

  if (parsed.length === 0) {
    return null;
  }

  return (
    <div className="mt-4 space-y-2">
      <div className="text-xs font-medium uppercase tracking-normal" style={{ color: "var(--text-muted)" }}>
        Goal items
      </div>
      <div className="space-y-1">
        {parsed.map((item, index) => {
          const record = item && typeof item === "object" ? item as JsonRecord : null;
          const title = stringField(record, "title") ?? stringField(record, "id") ?? `Item ${index + 1}`;
          const status = stringField(record, "status") ?? "pending";
          return (
            <div key={`${title}-${index}`} className="flex items-center justify-between gap-3 text-sm">
              <span className="truncate" style={{ color: "var(--text-secondary)" }}>{title}</span>
              <Pill label={status} status={status} />
            </div>
          );
        })}
      </div>
    </div>
  );
}

function DiffStats({ value }: { value: string | null }) {
  const stats = parseRecord(value);
  const files = numberField(stats, "filesChanged") ?? numberField(stats, "files_changed");
  const additions = numberField(stats, "additions");
  const deletions = numberField(stats, "deletions");

  if (files === null && additions === null && deletions === null) {
    return <span>Diff not recorded</span>;
  }

  return (
    <span>
      {files ?? 0} files, +{additions ?? 0} / -{deletions ?? 0}
    </span>
  );
}

function JudgeVerdictCard({ run }: { run: AutomationRun }) {
  const [expanded, setExpanded] = useState(false);
  const verdict = parseRecord(run.judgeVerdictJson);
  if (!verdict) {
    return null;
  }

  const decision = stringField(verdict, "decision") ?? "unknown";
  const reason = stringField(verdict, "reason");
  const confidence = numberField(verdict, "confidence");
  const nextRunPrompt = stringField(verdict, "nextRunPrompt");

  return (
    <div
      className="mt-3 rounded-md p-3"
      style={{
        backgroundColor: "var(--bg-hover)",
        borderColor: "var(--border-default)",
        borderStyle: "solid",
        borderWidth: "1px",
      }}
      data-testid={`automation-run-${run.id}-judge`}
    >
      <div className="flex flex-wrap items-center gap-2">
        <Pill label={`Judge: ${decision}`} status={decision} />
        {confidence !== null && (
          <span className="text-xs" style={{ color: "var(--text-muted)" }}>
            Confidence {Math.round(confidence * 100)}%
          </span>
        )}
      </div>
      {reason && (
        <p className="mt-2 text-sm" style={{ color: "var(--text-secondary)" }}>
          {reason}
        </p>
      )}
      {nextRunPrompt && (
        <div className="mt-2">
          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="gap-2"
            onClick={() => setExpanded((value) => !value)}
          >
            {expanded ? <ChevronUp className="h-4 w-4" /> : <ChevronDown className="h-4 w-4" />}
            {expanded ? "Hide next prompt" : "Show next prompt"}
          </Button>
          {expanded && (
            <div className="mt-2">
              <ExpandableText text={nextRunPrompt} maxLines={8} />
            </div>
          )}
        </div>
      )}
    </div>
  );
}

const RunTimelineItem = memo(function RunTimelineItem({
  run,
  projectId,
  onOpenRunConversation,
}: {
  run: AutomationRun;
  projectId: string | null;
  onOpenRunConversation?: (projectId: string, conversationId: string) => void;
}) {
  const canOpenConversation = Boolean(projectId && run.conversationId && onOpenRunConversation);
  const openConversation = useCallback(() => {
    if (projectId && run.conversationId) {
      onOpenRunConversation?.(projectId, run.conversationId);
    }
  }, [onOpenRunConversation, projectId, run.conversationId]);

  return (
    <div className="relative pl-6" data-testid={`automation-run-${run.id}`}>
      <div
        className="absolute left-0 top-1 h-3 w-3 rounded-full"
        style={{
          backgroundColor: "var(--accent-primary)",
          borderColor: "var(--app-content-bg)",
          borderStyle: "solid",
          borderWidth: "2px",
        }}
      />
      <div
        className="rounded-md p-4"
        style={{
          backgroundColor: "var(--bg-surface)",
          borderColor: "var(--border-default)",
          borderStyle: "solid",
          borderWidth: "1px",
        }}
      >
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="flex min-w-0 flex-wrap items-center gap-2">
            <span className="text-sm font-semibold" style={{ color: "var(--text-primary)" }}>
              Run {run.runIndex}
            </span>
            <Pill label={RUN_STATUS_LABELS[run.status]} status={run.status} />
            <Pill label={`Judge ${run.judgeState}`} status={run.judgeState} />
          </div>
          <span className="text-xs" style={{ color: "var(--text-muted)" }}>
            {formatDate(run.updatedAt)}
          </span>
        </div>

        <div className="mt-3 grid grid-cols-1 gap-3 text-sm md:grid-cols-2">
          <div>
            <div className="text-xs font-medium uppercase tracking-normal" style={{ color: "var(--text-muted)" }}>
              Prompt author
            </div>
            <div className="mt-1" style={{ color: "var(--text-secondary)" }}>
              {PROMPT_AUTHOR_LABELS[run.promptAuthor]}
            </div>
          </div>
          <div>
            <div className="text-xs font-medium uppercase tracking-normal" style={{ color: "var(--text-muted)" }}>
              Base
            </div>
            <div className="mt-1 truncate" style={{ color: "var(--text-secondary)" }}>
              {run.baseRefUsed || run.baseRefKind}
            </div>
          </div>
          <div>
            <div className="text-xs font-medium uppercase tracking-normal" style={{ color: "var(--text-muted)" }}>
              Conversation
            </div>
            <Button
              type="button"
              variant="link"
              className="h-auto p-0 text-sm"
              disabled={!canOpenConversation}
              onClick={openConversation}
            >
              {run.conversationId ? "Open conversation" : "Not started"}
            </Button>
          </div>
          <div>
            <div className="text-xs font-medium uppercase tracking-normal" style={{ color: "var(--text-muted)" }}>
              PR
            </div>
            {run.prUrl ? (
              <a
                href={run.prUrl}
                target="_blank"
                rel="noreferrer"
                className="mt-1 inline-flex items-center gap-1 text-sm text-[var(--accent-primary)]"
              >
                PR #{run.prNumber ?? "?"}
                <ExternalLink className="h-3 w-3" aria-hidden="true" />
              </a>
            ) : (
              <div className="mt-1" style={{ color: "var(--text-secondary)" }}>
                {run.prNumber ? `PR #${run.prNumber}` : "Not published"}
              </div>
            )}
          </div>
          <div>
            <div className="text-xs font-medium uppercase tracking-normal" style={{ color: "var(--text-muted)" }}>
              Diff
            </div>
            <div className="mt-1" style={{ color: "var(--text-secondary)" }}>
              <DiffStats value={run.diffStatsJson} />
            </div>
          </div>
          <div>
            <div className="text-xs font-medium uppercase tracking-normal" style={{ color: "var(--text-muted)" }}>
              Finished
            </div>
            <div className="mt-1" style={{ color: "var(--text-secondary)" }}>
              {formatDate(run.finishedAt)}
            </div>
          </div>
        </div>

        <div className="mt-4 space-y-4">
          <div>
            <div className="mb-2 text-xs font-medium uppercase tracking-normal" style={{ color: "var(--text-muted)" }}>
              Run prompt
            </div>
            <ExpandableText text={run.runPrompt} />
          </div>
          <div>
            <div className="mb-2 text-xs font-medium uppercase tracking-normal" style={{ color: "var(--text-muted)" }}>
              Agent summary
            </div>
            <ExpandableText text={run.agentSummary} maxLines={6} />
          </div>
        </div>
        <JudgeVerdictCard run={run} />
      </div>
    </div>
  );
});

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
  const stopMutation = useMutation({
    mutationFn: () => automationsApi.stop(automationId),
    onSuccess: () => {
      invalidate();
      toast.success("Automation stopped");
    },
    onError: () => toast.error("Failed to stop automation"),
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
      invalidate();
      toast.success("Automation deleted");
      onBack();
    },
    onError: () => toast.error("Failed to delete automation"),
  });

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
  const skipJudgeRun = isSignalTerminalUnjudged(latest) ? latest : null;
  const actionPending = pauseMutation.isPending
    || resumeMutation.isPending
    || stopMutation.isPending
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

  const handleDelete = async () => {
    const confirmed = await confirm({
      title: "Delete automation?",
      description: "Delete removes the terminal automation and its run history.",
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
            </div>
            <div className="mt-1 text-xs" style={{ color: "var(--text-muted)" }}>
              {projectName ?? projectId ?? automation.projectId}
            </div>
          </div>
        </div>
        <div className="flex items-center gap-2">
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
            variant="outline"
            disabled={actionPending || isAutomationTerminal(automation.status) || automation.status === "draft"}
            onClick={() => void handleRunNow()}
          >
            <PlayCircle className="h-4 w-4" />
          </TooltipIconButton>
          <TooltipIconButton
            label="Stop automation"
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
                disabled={!isAutomationTerminal(automation.status)}
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
        <div className="grid gap-4 lg:grid-cols-[minmax(0,0.8fr)_minmax(0,1.2fr)]">
          <div className="space-y-4">
            <Section title="Goal" testId="automation-goal-card">
              <ExpandableText text={automation.goalPrompt} />
              <GoalItems value={automation.goalItemsJson} />
            </Section>
            <Section title="Inputs">
              <SourcePrInput automation={automation} />
            </Section>
            <Section title="Config">
              <KeyValueList
                items={[
                  ["Mode / model", formatMode(automation)],
                  ["Base", formatBase(automation)],
                  ["Chain mode", automation.chainMode],
                  ["Completion signal", automation.completionSignal],
                  ["Max runs", `${runs.length} / ${automation.maxRuns}`],
                  ["Max failures", automation.maxConsecutiveFailures],
                  ["Input tokens", formatNumber(usage.inputTokens)],
                  ["Output tokens", formatNumber(usage.outputTokens)],
                  ["Cache tokens", formatNumber(usage.cacheCreationTokens + usage.cacheReadTokens)],
                  ["Estimated cost", formatEstimatedUsd(usage.estimatedUsd)],
                  ["Created", formatDate(automation.createdAt)],
                  ["Updated", formatDate(automation.updatedAt)],
                ]}
              />
              {automation.pausedReasonCode && (
                <div className="mt-4 rounded-md p-3 text-sm" style={{
                  backgroundColor: "var(--bg-hover)",
                  color: "var(--text-secondary)",
                }}>
                  Paused: {automation.pausedReasonCode}
                  {automation.pausedReasonDetail ? ` - ${automation.pausedReasonDetail}` : ""}
                </div>
              )}
            </Section>
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
                    projectId={projectId}
                    {...(onOpenRunConversation ? { onOpenRunConversation } : {})}
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
