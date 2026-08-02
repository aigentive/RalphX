import { Suspense, useCallback, useEffect, useMemo, useState } from "react";
import { Plus, Workflow } from "lucide-react";

import type { Automation } from "@/api/automations";
import { useAfterPaintMounted } from "@/components/agents/agentDeferredFrame";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import type { AutomationRunOpenTarget } from "@/components/automations/automationRunNavigation";
import { preloadAutomationDetailView } from "@/components/automations/preloadAutomationDetailView";
import { useAutomationsList } from "@/hooks/useAutomations";
import { useAgentGate } from "@/hooks/useAgentGate";
import { withAlpha } from "@/lib/theme-colors";
import { lazyWithRetry } from "@/lib/lazy-with-retry";
import { AutomationListGroup } from "./AutomationListGroup";
import { AutomationListRow } from "./AutomationListRow";
import { AutomationListToolbar } from "./AutomationListToolbar";
import {
  automationListFilterCounts,
  automationListSummary,
  filterAndGroupAutomations,
  type AutomationListFilter,
} from "./automationListPresentation";

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

const LazyAutomationDetailView = lazyWithRetry(() => preloadAutomationDetailView());
const EMPTY_AUTOMATIONS: Automation[] = [];

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

function EmptyAutomations({
  onNewAutomation,
  newAutomationDisabled = false,
  newAutomationReason = null,
}: {
  onNewAutomation?: () => void;
  newAutomationDisabled?: boolean;
  newAutomationReason?: string | null;
}) {
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
        disabled={!onNewAutomation || newAutomationDisabled}
        title={newAutomationReason ?? undefined}
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
  const createGate = useAgentGate("automationCreate");
  const [localSelectedAutomationId, setLocalSelectedAutomationId] = useState<string | null>(null);
  const [filter, setFilter] = useState<AutomationListFilter>("all");
  const [searchText, setSearchText] = useState("");
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
  const rows = automations.data ?? EMPTY_AUTOMATIONS;
  const filterCounts = useMemo(() => automationListFilterCounts(rows), [rows]);
  const groupedRows = useMemo(
    () => filterAndGroupAutomations(rows, filter, searchText),
    [filter, rows, searchText],
  );
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
    setFilter("all");
    setSearchText("");
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
            {automationListSummary(projectLabel, rows)}
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
            disabled={!projectId || !onNewAutomation || createGate.gated}
            title={createGate.reason ?? undefined}
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
          <EmptyAutomations
            {...(onNewAutomation ? { onNewAutomation } : {})}
            newAutomationDisabled={createGate.gated}
            newAutomationReason={createGate.reason}
          />
        ) : (
          <div className="space-y-5" data-testid="automations-list">
            <AutomationListToolbar
              activeFilter={filter}
              counts={filterCounts}
              searchText={searchText}
              onFilterChange={setFilter}
              onSearchTextChange={setSearchText}
            />
            {groupedRows.length === 0 ? (
              <div
                className="rounded-lg px-4 py-10 text-center text-sm"
                style={{
                  backgroundColor: "var(--bg-surface, #1e1e23)",
                  borderColor: "var(--border-subtle, #2e2e36)",
                  borderStyle: "solid",
                  borderWidth: "1px",
                  color: "var(--text-muted, #8e8e96)",
                }}
                data-testid="automations-filter-empty-state"
              >
                No automations match this filter.
              </div>
            ) : groupedRows.map((group) => (
              <AutomationListGroup
                key={group.id}
                title={group.label}
                hint={group.hint}
                count={group.automations.length}
                testId={`automations-group-${group.id}`}
              >
                {group.automations.map((automation, index) => (
                  <AutomationListRow
                    key={automation.id}
                    automation={automation}
                    divided={index > 0}
                    onOpenAutomation={handleOpenAutomation}
                  />
                ))}
              </AutomationListGroup>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
