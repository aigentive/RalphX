import { lazy, memo, Suspense, useCallback, useEffect, useState } from "react";
import { ChevronRight, Plus, Workflow } from "lucide-react";

import type { Automation, AutomationRun } from "@/api/automations";
import { useAfterPaintMounted } from "@/components/agents/agentDeferredFrame";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import type { AutomationRunOpenTarget } from "@/components/automations/automationRunNavigation";
import {
  getAutomationRunView,
  latestRun,
} from "@/components/automations/automationStage";
import { preloadAutomationDetailView } from "@/components/automations/preloadAutomationDetailView";
import { useAutomationDetail, useAutomationsList } from "@/hooks/useAutomations";
import { withAlpha } from "@/lib/theme-colors";
import { Pill, statusDotColor } from "./automationDetailShared";

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
  active: "Active",
  paused: "Paused",
  completed: "Completed",
  stopped: "Stopped",
};

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

function formatSecondaryLine(automation: Automation, stageLabel: string): string {
  if (automation.status === "draft") {
    return `Draft setup · ${formatGoalMetadata(automation)}`;
  }
  const phaseCount = parsePhaseCount(automation.goalItemsJson);
  const segments = [
    stageLabel === "Paused" || stageLabel === "Stopped" ? null : stageLabel,
    phaseCount > 0
      ? `${phaseCount} ${phaseCount === 1 ? "phase" : "phases"}`
      : null,
    automation.runMode,
    automation.modelId,
  ];
  return segments.filter((segment): segment is string => Boolean(segment)).join(" · ");
}

function formatLastRun(automation: Automation, run: AutomationRun | null): string {
  if (!run) {
    return automation.status === "draft" ? "Not started" : "No runs yet";
  }
  const pr = run.prNumber ? ` · PR #${run.prNumber}` : "";
  const view = getAutomationRunView(automation, run);
  return `Run ${run.runIndex} · ${view.statusLabel}${pr}`;
}

function AutomationsListSkeleton() {
  return (
    <div
      className="overflow-hidden rounded-lg"
      style={{
        backgroundColor: "var(--bg-surface, #1e1e23)",
        borderColor: "var(--border-subtle, #2e2e36)",
        borderStyle: "solid",
        borderWidth: "1px",
      }}
      data-testid="automations-list-skeleton"
    >
      {[0, 1, 2].map((index) => (
        <div
          key={index}
          className="flex min-h-[64px] flex-wrap items-center gap-4 px-4 py-3 md:flex-nowrap"
          style={{
            ...(index > 0
              ? {
                  borderTopColor: "var(--border-subtle, #2e2e36)",
                  borderTopStyle: "solid",
                  borderTopWidth: "1px",
                }
              : {}),
          }}
        >
          <Skeleton className="h-2 w-2 shrink-0 rounded-full" />
          <div className="min-w-0 flex-1 space-y-2">
            <Skeleton className="h-4 w-40 max-w-full" />
            <Skeleton className="h-3 w-64 max-w-full" />
          </div>
          <div className="ml-6 flex basis-[calc(100%_-_1.5rem)] shrink-0 items-center gap-3 md:ml-0 md:basis-auto">
            <Skeleton className="h-5 w-20 rounded-full" />
            <Skeleton className="h-3 w-20" />
            <Skeleton className="hidden h-3 w-32 md:block" />
            <Skeleton className="hidden h-4 w-4 md:block" />
          </div>
        </div>
      ))}
    </div>
  );
}

const AutomationRow = memo(function AutomationRow({
  automation,
  divided,
  onOpenAutomation,
}: {
  automation: Automation;
  divided: boolean;
  onOpenAutomation?: (automationId: string) => void;
}) {
  const detail = useAutomationDetail(automation.id);
  const runs = detail.data?.runs;
  const run = latestRun(runs ?? []);
  const runView = getAutomationRunView(automation, run);
  const runsCount = runs?.length ?? 0;
  const secondaryLine = formatSecondaryLine(automation, runView.stageLabel);
  const showProgress =
    automation.status !== "draft" &&
    runsCount > 0 &&
    automation.maxRuns > 0 &&
    automation.maxRuns <= 200;
  const progressPercent = Math.min(100, Math.max(0, (runsCount / Math.max(automation.maxRuns, 1)) * 100));
  const handleOpen = useCallback(() => onOpenAutomation?.(automation.id), [automation.id, onOpenAutomation]);

  return (
    <button
      type="button"
      className="group flex min-h-[64px] w-full flex-wrap items-center gap-4 px-4 py-3 text-left transition-colors hover:bg-[var(--bg-hover)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-[var(--accent-primary)] disabled:cursor-default disabled:hover:bg-transparent md:flex-nowrap"
      style={{
        ...(divided
          ? {
              borderTopColor: "var(--border-subtle, #2e2e36)",
              borderTopStyle: "solid",
              borderTopWidth: "1px",
            }
          : {}),
      }}
      disabled={!onOpenAutomation}
      onClick={handleOpen}
      data-testid={`automation-row-${automation.id}`}
    >
      <span
        aria-hidden="true"
        className="h-2 w-2 shrink-0 rounded-full"
        style={{ backgroundColor: statusDotColor(automation.status) }}
        data-testid={`automation-row-${automation.id}-status-dot`}
      />
      <div className="min-w-0 flex-1">
        <div
          className="truncate text-sm font-medium"
          style={{
            color: automation.name.trim()
              ? "var(--text-primary, #f2f2f4)"
              : "var(--text-muted, #8e8e96)",
          }}
        >
          {automation.name.trim() || "Untitled automation"}
        </div>
        <div
          className="mt-0.5 truncate text-xs"
          style={{ color: "var(--text-muted, #8e8e96)" }}
          data-testid={`automation-row-${automation.id}-metadata`}
        >
          {secondaryLine}
        </div>
      </div>
      <div className="ml-6 flex basis-[calc(100%_-_1.5rem)] shrink-0 items-center gap-3 md:ml-0 md:basis-auto">
        <div className="flex w-[92px] justify-end">
          <Pill
            label={STATUS_LABELS[automation.status]}
            status={automation.status}
            live={automation.status === "active" && runView.isOpen}
          />
        </div>
        <div className="flex min-w-[76px] items-center justify-end gap-2">
          {automation.status === "draft" ? (
            <span className="text-xs" style={{ color: "var(--text-muted, #8e8e96)" }}>
              —
            </span>
          ) : (
            <>
              <span
                className="text-xs tabular-nums"
                style={{ color: "var(--text-secondary, #c7c7cc)" }}
              >
                {runsCount}/{automation.maxRuns}
              </span>
              {showProgress ? (
                <div
                  className="h-1 w-12 overflow-hidden rounded-full"
                  style={{ backgroundColor: "var(--bg-hover, #2a2a31)" }}
                  aria-hidden="true"
                  data-testid={`automation-row-${automation.id}-runs-progress`}
                >
                  <div
                    className="h-full rounded-full"
                    style={{
                      backgroundColor:
                        automation.status === "active" && runView.isOpen
                          ? "var(--accent-primary)"
                          : "var(--text-subtle, #6a6a72)",
                      width: `${progressPercent}%`,
                    }}
                    data-testid={`automation-row-${automation.id}-runs-progress-fill`}
                  />
                </div>
              ) : null}
            </>
          )}
        </div>
        <div
          className="hidden w-[150px] truncate text-right text-xs md:block"
          style={{
            color: runView.statusTone === "error"
              ? "var(--status-error, #dd3c3c)"
              : "var(--text-muted, #8e8e96)",
          }}
        >
          {detail.isLoading ? "Loading runs…" : formatLastRun(automation, run)}
        </div>
        <ChevronRight
          className="hidden h-4 w-4 text-[var(--text-subtle)] transition-colors group-hover:text-[var(--text-secondary)] md:block"
          aria-hidden="true"
        />
      </div>
    </button>
  );
});

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
            {projectLabel} · {rows.length} automations
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
          <div
            className="overflow-hidden rounded-lg"
            style={{
              backgroundColor: "var(--bg-surface, #1e1e23)",
              borderColor: "var(--border-subtle, #2e2e36)",
              borderStyle: "solid",
              borderWidth: "1px",
            }}
            data-testid="automations-list"
          >
            {rows.map((automation, index) => (
              <AutomationRow
                key={automation.id}
                automation={automation}
                divided={index > 0}
                onOpenAutomation={handleOpenAutomation}
              />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
