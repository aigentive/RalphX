import { lazy, Suspense, useCallback, useEffect, useState } from "react";
import { ChevronRight, Plus, Workflow } from "lucide-react";

import type { Automation, AutomationRun } from "@/api/automations";
import { useAfterPaintMounted } from "@/components/agents/agentDeferredFrame";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { StatusPill, type StatusPillTone } from "@/components/ui/status-pill";
import type { AutomationRunOpenTarget } from "@/components/automations/automationRunNavigation";
import {
  getAutomationRunView,
  latestRun,
} from "@/components/automations/automationStage";
import { preloadAutomationDetailView } from "@/components/automations/preloadAutomationDetailView";
import { useAutomationDetail, useAutomationsList } from "@/hooks/useAutomations";
import { withAlpha } from "@/lib/theme-colors";

interface AutomationsViewProps {
  projectId: string | null;
  projectName?: string | null;
  projectOptions?: Array<{ id: string; name: string }>;
  onProjectChange?: (projectId: string) => void;
  onNewAutomation?: () => void;
  selectedAutomationId?: string | null;
  onSelectedAutomationChange?: (automationId: string | null) => void;
  onOpenAutomation?: (automationId: string) => void;
  onOpenRunConversation?: (projectId: string, conversationId: string) => void;
  onOpenAutomationRun?: (target: AutomationRunOpenTarget) => void;
}

const LazyAutomationDetailView = lazy(() => preloadAutomationDetailView());

const STATUS_LABELS: Record<Automation["status"], string> = {
  draft: "Draft",
  active: "Approved",
  paused: "Paused",
  completed: "Completed",
  stopped: "Stopped",
};

function formatBase(automation: Automation): string {
  return (automation.baseDisplayName ?? automation.baseRef) || automation.baseRefKind;
}

function parsePhaseCount(value: string | null): number {
  if (!value?.trim()) {
    return 0;
  }
  try {
    const parsed = JSON.parse(value) as unknown;
    return Array.isArray(parsed) ? parsed.length : 0;
  } catch {
    return 0;
  }
}

function formatGoalMetadata(automation: Automation): string {
  const phaseCount = parsePhaseCount(automation.goalItemsJson);
  const goalState = automation.goalPrompt.trim() ? "Goal set" : "No goal";
  const phaseState =
    phaseCount === 0 ? "No phases" : `${phaseCount} ${phaseCount === 1 ? "phase" : "phases"}`;
  const firstRunState = automation.firstRunPrompt?.trim() ? "First run ready" : "No first run";
  return `${goalState} · ${phaseState} · ${firstRunState}`;
}

function formatLastRun(automation: Automation, run: AutomationRun | null): string {
  if (!run) {
    return "No runs yet";
  }
  const pr = run.prNumber ? ` · PR #${run.prNumber}` : "";
  const view = getAutomationRunView(automation, run);
  return `Run ${run.runIndex} ${view.statusLabel}${pr}`;
}

function AutomationsListSkeleton() {
  return (
    <div className="space-y-2" data-testid="automations-list-skeleton">
      {[0, 1, 2].map((index) => (
        <div
          key={index}
          className="grid min-h-[58px] grid-cols-1 items-center gap-2 rounded-md px-4 py-3 md:grid-cols-[1.35fr_0.55fr_0.75fr_0.75fr_1fr_0.5fr_0.9fr_1fr_24px] md:gap-3 md:py-0"
          style={{
            backgroundColor: "var(--bg-surface)",
            borderColor: "var(--border-default)",
            borderStyle: "solid",
            borderWidth: "1px",
          }}
        >
          <Skeleton className="h-4 w-40" />
          <Skeleton className="h-5 w-20" />
          <Skeleton className="h-4 w-28" />
          <Skeleton className="h-4 w-24" />
          <Skeleton className="h-4 w-32" />
          <Skeleton className="h-4 w-16" />
          <Skeleton className="h-4 w-36" />
          <Skeleton className="h-4 w-36" />
          <Skeleton className="h-5 w-5" />
        </div>
      ))}
    </div>
  );
}

function statusPillTone(status: Automation["status"]): StatusPillTone {
  switch (status) {
    case "active":
      return "success";
    case "paused":
      return "warning";
    case "completed":
      return "accent";
    default:
      return "neutral";
  }
}

function AutomationStatusPill({ status }: { status: Automation["status"] }) {
  return (
    <StatusPill label={STATUS_LABELS[status]} size="md" tone={statusPillTone(status)} />
  );
}

function AutomationRow({
  automation,
  projectName,
  onOpenAutomation,
}: {
  automation: Automation;
  projectName: string;
  onOpenAutomation?: (automationId: string) => void;
}) {
  const detail = useAutomationDetail(automation.id);
  const runs = detail.data?.runs;
  const run = latestRun(runs ?? []);
  const runView = getAutomationRunView(automation, run);
  const canOpen = Boolean(onOpenAutomation);

  return (
    <button
      type="button"
      className="grid min-h-[58px] w-full grid-cols-1 items-center gap-2 rounded-md px-4 py-3 text-left transition-colors hover:bg-[var(--bg-hover)] disabled:cursor-default disabled:hover:bg-transparent md:grid-cols-[1.35fr_0.55fr_0.75fr_0.75fr_1fr_0.5fr_0.9fr_1fr_24px] md:gap-3 md:py-0"
      style={{
        backgroundColor: "var(--bg-surface)",
        borderColor: "var(--border-default)",
        borderStyle: "solid",
        borderWidth: "1px",
      }}
      disabled={!canOpen}
      onClick={() => onOpenAutomation?.(automation.id)}
      data-testid={`automation-row-${automation.id}`}
    >
      <div className="min-w-0">
        <div className="truncate text-sm font-semibold" style={{ color: "var(--text-primary)" }}>
          {automation.name}
        </div>
        <div
          className="mt-1 truncate text-[0.6875rem]"
          style={{ color: "var(--text-muted)" }}
          data-testid={`automation-row-${automation.id}-metadata`}
        >
          {formatGoalMetadata(automation)}
        </div>
      </div>
      <AutomationStatusPill status={automation.status} />
      <div className="truncate text-sm" style={{ color: "var(--text-secondary)" }}>
        {projectName}
      </div>
      <div className="truncate text-xs" style={{ color: "var(--text-muted)" }}>
        {formatBase(automation)}
      </div>
      <div className="truncate text-xs" style={{ color: "var(--text-muted)" }}>
        <span className="font-medium" style={{ color: "var(--text-secondary)" }}>
          {automation.runMode}
        </span>
        <span> · {automation.providerHarness}/{automation.modelId}</span>
        {automation.logicalEffort ? <span>/{automation.logicalEffort}</span> : null}
      </div>
      <div className="truncate text-sm" style={{ color: "var(--text-secondary)" }}>
        {detail.isLoading ? "..." : `${runs?.length ?? 0} / ${automation.maxRuns}`}
      </div>
      <div className="truncate text-xs" style={{ color: "var(--text-muted)" }}>
        {detail.isLoading ? "Loading runs..." : formatLastRun(automation, run)}
      </div>
      <div className="truncate text-xs font-medium" style={{ color: "var(--text-secondary)" }}>
        {detail.isLoading ? "Hydrating status" : runView.stageLabel}
      </div>
      <ChevronRight className="h-4 w-4" style={{ color: "var(--text-muted)" }} aria-hidden="true" />
    </button>
  );
}

function EmptyAutomations({ onNewAutomation }: { onNewAutomation?: () => void }) {
  return (
    <div
      className="flex min-h-[360px] flex-col items-center justify-center rounded-md px-6 py-10 text-center"
      style={{
        backgroundColor: "var(--bg-surface)",
        borderColor: "var(--border-default)",
        borderStyle: "solid",
        borderWidth: "1px",
      }}
      data-testid="automations-empty-state"
    >
      <div
        className="grid h-14 w-14 place-items-center rounded-full"
        style={{ backgroundColor: withAlpha("var(--accent-primary)", 14) }}
        aria-hidden="true"
      >
        <Workflow className="h-7 w-7" style={{ color: "var(--accent-primary)" }} />
      </div>
      <h2 className="mt-5 text-lg font-semibold" style={{ color: "var(--text-primary)" }}>
        No automations yet
      </h2>
      <p className="mt-2 max-w-[520px] text-sm leading-6" style={{ color: "var(--text-secondary)" }}>
        Set a goal once; RalphX runs an agent, waits for the PR to merge, lets a judge choose the next prompt, and repeats until the goal is done.
      </p>
      <Button
        type="button"
        className="mt-6 gap-2"
        onClick={onNewAutomation}
        disabled={!onNewAutomation}
        data-testid="automations-empty-new-button"
      >
        <Plus className="h-4 w-4" />
        New automation
      </Button>
    </div>
  );
}

function AutomationDetailShell({ onBack }: { onBack: () => void }) {
  return (
    <div
      className="flex h-full min-h-0 flex-col"
      style={{ backgroundColor: "var(--app-content-bg)" }}
      data-testid="automation-detail-shell"
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
        <Button type="button" variant="ghost" onClick={onBack}>
          Back
        </Button>
        <div className="flex gap-2">
          {[0, 1, 2].map((index) => (
            <Skeleton key={index} className="h-8 w-8 rounded-md" />
          ))}
        </div>
      </div>
      <div className="grid gap-4 p-6 lg:grid-cols-[minmax(0,0.8fr)_minmax(0,1.2fr)]">
        <Skeleton className="h-56 rounded-md" />
        <Skeleton className="h-80 rounded-md" />
      </div>
    </div>
  );
}

export function AutomationsView({
  projectId,
  projectName,
  projectOptions = [],
  onProjectChange,
  onNewAutomation,
  selectedAutomationId,
  onSelectedAutomationChange,
  onOpenAutomation,
  onOpenRunConversation,
  onOpenAutomationRun,
}: AutomationsViewProps) {
  const [localSelectedAutomationId, setLocalSelectedAutomationId] = useState<string | null>(null);
  const isSelectionControlled = selectedAutomationId !== undefined;
  const activeAutomationId = isSelectionControlled
    ? selectedAutomationId
    : localSelectedAutomationId;
  const setSelectedAutomation = useCallback(
    (automationId: string | null) => {
      if (isSelectionControlled) {
        onSelectedAutomationChange?.(automationId);
        return;
      }
      setLocalSelectedAutomationId(automationId);
    },
    [isSelectionControlled, onSelectedAutomationChange],
  );
  const afterPaint = useAfterPaintMounted(Boolean(projectId));
  const automations = useAutomationsList(projectId, { enabled: afterPaint });
  const projectLabel = projectName ?? projectId ?? "Current project";
  const rows = automations.data ?? [];
  const showSkeleton = Boolean(projectId) && (!afterPaint || automations.isLoading);
  const handleOpenAutomation = useCallback((automationId: string) => {
    setSelectedAutomation(automationId);
    onOpenAutomation?.(automationId);
  }, [onOpenAutomation, setSelectedAutomation]);
  const handleBackToList = useCallback(() => {
    setSelectedAutomation(null);
  }, [setSelectedAutomation]);

  useEffect(() => {
    if (!isSelectionControlled) {
      setLocalSelectedAutomationId(null);
    }
  }, [isSelectionControlled, projectId]);

  if (activeAutomationId) {
    return (
      <Suspense fallback={<AutomationDetailShell onBack={handleBackToList} />}>
        <LazyAutomationDetailView
          automationId={activeAutomationId}
          projectId={projectId}
          projectName={projectName ?? null}
          onBack={handleBackToList}
          {...(onOpenRunConversation ? { onOpenRunConversation } : {})}
          {...(onOpenAutomationRun ? { onOpenAutomationRun } : {})}
        />
      </Suspense>
    );
  }

  return (
    <div
      className="flex h-full min-h-0 flex-col"
      style={{ backgroundColor: "var(--app-content-bg)" }}
      data-testid="automations-view"
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
        <div className="min-w-0">
          <h1 className="truncate text-xl font-semibold" style={{ color: "var(--text-primary)" }}>
            Automations
          </h1>
          <div className="mt-1 text-xs font-medium" style={{ color: "var(--text-muted)" }}>
            {projectLabel}
          </div>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <label className="sr-only" htmlFor="automations-project-select">
            Project
          </label>
          <select
            id="automations-project-select"
            className="h-9 min-w-[180px] rounded-md px-3 text-sm font-medium outline-none"
            style={{
              backgroundColor: "var(--bg-surface)",
              borderColor: "var(--border-default)",
              borderStyle: "solid",
              borderWidth: "1px",
              color: "var(--text-primary)",
            }}
            value={projectId ?? ""}
            onChange={(event) => onProjectChange?.(event.target.value)}
            disabled={!onProjectChange || projectOptions.length === 0}
            data-testid="automations-project-select"
          >
            {projectOptions.length === 0 ? (
              <option value={projectId ?? ""}>{projectLabel}</option>
            ) : (
              <>
                {!projectId ? <option value="">Select project</option> : null}
                {projectOptions.map((project) => (
                  <option key={project.id} value={project.id}>
                    {project.name}
                  </option>
                ))}
              </>
            )}
          </select>
          <Button
            type="button"
            className="gap-2"
            onClick={onNewAutomation}
            disabled={!projectId || !onNewAutomation}
            data-testid="automations-new-button"
          >
            <Plus className="h-4 w-4" />
            New automation
          </Button>
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-auto p-5">
        {!projectId ? (
          <EmptyAutomations />
        ) : showSkeleton ? (
          <AutomationsListSkeleton />
        ) : automations.isError ? (
          <div className="rounded-md p-4 text-sm" style={{ color: "var(--status-error)" }} data-testid="automations-error-state">
            Could not load automations.
          </div>
        ) : rows.length === 0 ? (
          <EmptyAutomations {...(onNewAutomation ? { onNewAutomation } : {})} />
        ) : (
          <div className="space-y-2" data-testid="automations-list">
            <div
              className="hidden grid-cols-[1.35fr_0.55fr_0.75fr_0.75fr_1fr_0.5fr_0.9fr_1fr_24px] gap-3 px-4 text-xs font-semibold uppercase tracking-normal md:grid"
              style={{ color: "var(--text-muted)" }}
              aria-hidden="true"
            >
              <span>Name</span>
              <span>Status</span>
              <span>Project</span>
              <span>Base</span>
              <span>Mode / model</span>
              <span>Runs</span>
              <span>Last run</span>
              <span>Next action</span>
              <span />
            </div>
            {rows.map((automation) => (
              <AutomationRow
                key={automation.id}
                automation={automation}
                projectName={projectLabel}
                onOpenAutomation={handleOpenAutomation}
              />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
