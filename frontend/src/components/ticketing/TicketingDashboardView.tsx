import { useEffect, useMemo, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";

import type { AgentConversationWorkspaceMode } from "@/api/chat";
import type {
  ListTicketsInput,
  TicketDeepLink,
  TicketFiltersInput,
  TicketingColumn,
  TicketSummary,
  TicketTransitionOption,
} from "@/api/ticketing";
import type { Project } from "@/types/project";
import {
  fetchTicketTransitionsForMove,
  findTicketTransitionForColumn,
  flattenTicketPages,
  useRefreshTickets,
  useStartWorkFromTicket,
  useTicketAssociations,
  useTicketDetail,
  useTicketingMutations,
  useTicketingColumns,
  useTicketingContainers,
  useTicketingProviders,
  useTicketTransitions,
  useTickets,
} from "@/hooks/useTicketing";
import { useProjects } from "@/hooks/useProjects";
import { useTicketingStore } from "@/stores/ticketingStore";
import { useChatStore } from "@/stores/chatStore";
import { useAgentSessionStore } from "@/stores/agentSessionStore";
import { useUiStore } from "@/stores/uiStore";
import { formatRelativeTime } from "@/lib/formatters";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

import { ProviderSwitcher } from "./ProviderSwitcher";
import { TicketDetailSheet } from "./TicketDetailSheet";
import { TicketFilterBar } from "./TicketFilterBar";
import { TicketingStatePanel } from "./TicketingStatePanel";
import { TicketKanbanShell, TicketKanbanView, TicketListView } from "./TicketViews";
import {
  distinctAssigneeNames,
  filterTicketsByAssignee,
  isTicketUpdatedSince,
  ticketRefKey,
} from "./ticketing-read-state";
import { providerLabel, ticketKey } from "./ticketing-utils";
import { useAfterPaint } from "./useAfterPaint";

interface TicketingDashboardViewProps {
  projectId: string;
  onNavigateToAssociation?: ((deepLink: TicketDeepLink) => void) | undefined;
}

function toTicketFilters(filters: ReturnType<typeof useTicketingStore.getState>["filters"]): TicketFiltersInput | undefined {
  // Assignee is filtered client-side (see filterTicketsByAssignee), so it is not
  // forwarded to the provider search here.
  const next: TicketFiltersInput = {
    ...(filters.text.trim() && { text: filters.text.trim() }),
    ...(filters.stateIds.length > 0 && { stateIds: filters.stateIds }),
    ...(filters.labels.length > 0 && { labels: filters.labels }),
  };
  return Object.keys(next).length > 0 ? next : undefined;
}

function isProviderReadable(status: string | undefined): boolean {
  return status === "connected";
}

function columnsFromTickets(tickets: TicketSummary[]): TicketingColumn[] {
  const byStateId = new Map<string, TicketingColumn>();
  for (const ticket of tickets) {
    if (byStateId.has(ticket.state.id)) {
      continue;
    }
    byStateId.set(ticket.state.id, {
      id: ticket.state.id,
      name: ticket.state.name,
      category: ticket.state.category,
      order: byStateId.size,
      ...(ticket.state.color ? { color: ticket.state.color } : {}),
    });
  }
  return Array.from(byStateId.values());
}

function mergeProviderAndTicketColumns(
  providerColumns: TicketingColumn[],
  ticketColumns: TicketingColumn[],
): TicketingColumn[] {
  if (providerColumns.length === 0) {
    return ticketColumns;
  }
  const merged = providerColumns
    .sort((left, right) => left.order - right.order);
  const providerColumnIds = new Set(merged.map((column) => column.id));
  for (const column of ticketColumns) {
    if (!providerColumnIds.has(column.id)) {
      merged.push({ ...column, order: merged.length });
    }
  }
  return merged;
}

function columnsFromTransitions(transitions: TicketTransitionOption[]): TicketingColumn[] {
  return transitions.map((transition, index) => ({
    id: transition.toStateId,
    name: transition.name,
    category: transition.category,
    order: index,
  }));
}

function containerLabelsForProvider(provider: string | null): {
  containerLabel: string;
  allContainersLabel: string;
} {
  if (provider === "linear") {
    return { containerLabel: "Project", allContainersLabel: "All projects" };
  }
  if (provider === "jira") {
    return { containerLabel: "Board", allContainersLabel: "All boards" };
  }
  return { containerLabel: "Container", allContainersLabel: "All containers" };
}

const START_WORK_MODES: Array<{
  value: AgentConversationWorkspaceMode;
  label: string;
}> = [
  { value: "edit", label: "Agent" },
  { value: "plan", label: "Plan" },
  { value: "ideation", label: "Ideation" },
  { value: "chat", label: "Chat" },
];

interface StartWorkSelection {
  projectId: string;
  mode: AgentConversationWorkspaceMode;
}

function StartWorkDialog({
  open,
  ticket,
  projects,
  selected,
  isPending,
  onSelectionChange,
  onConfirm,
  onClose,
}: {
  open: boolean;
  ticket: TicketSummary | null;
  projects: Project[];
  selected: StartWorkSelection;
  isPending: boolean;
  onSelectionChange: (selection: StartWorkSelection) => void;
  onConfirm: () => void;
  onClose: () => void;
}) {
  return (
    <Dialog open={open} onOpenChange={(nextOpen) => !nextOpen && onClose()}>
      <DialogContent className="sm:max-w-[460px]">
        <DialogHeader>
          <DialogTitle>Start RalphX Work</DialogTitle>
          <DialogDescription>
            Choose where to start the linked conversation for {ticket ? ticketKey(ticket.ref) : "this ticket"}.
          </DialogDescription>
        </DialogHeader>

        <div className="grid gap-4 py-2">
          <label className="grid gap-1.5 text-sm">
            Project
            <select
              className="h-9 rounded-md px-2 text-sm outline-none"
              value={selected.projectId}
              onChange={(event) =>
                onSelectionChange({ ...selected, projectId: event.target.value })
              }
              style={{
                backgroundColor: "var(--bg-elevated)",
                borderColor: "var(--border-default)",
                borderStyle: "solid",
                borderWidth: "1px",
                color: "var(--text-primary)",
              }}
            >
              {projects.map((project) => (
                <option key={project.id} value={project.id}>
                  {project.name}
                </option>
              ))}
            </select>
          </label>

          <label className="grid gap-1.5 text-sm">
            Conversation type
            <select
              className="h-9 rounded-md px-2 text-sm outline-none"
              value={selected.mode}
              onChange={(event) =>
                onSelectionChange({
                  ...selected,
                  mode: event.target.value as AgentConversationWorkspaceMode,
                })
              }
              style={{
                backgroundColor: "var(--bg-elevated)",
                borderColor: "var(--border-default)",
                borderStyle: "solid",
                borderWidth: "1px",
                color: "var(--text-primary)",
              }}
            >
              {START_WORK_MODES.map((mode) => (
                <option key={mode.value} value={mode.value}>
                  {mode.label}
                </option>
              ))}
            </select>
          </label>
        </div>

        <DialogFooter>
          <Button type="button" variant="ghost" onClick={onClose} disabled={isPending}>
            Cancel
          </Button>
          <Button
            type="button"
            onClick={onConfirm}
            disabled={isPending || !selected.projectId}
          >
            {isPending ? "Starting" : "Start"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

interface TicketingStatusNotice {
  id: string;
  tone: "warning" | "error";
  message: string;
  detail?: string | undefined;
}

function queryErrorDetail(error: unknown, fallback: string): string {
  return error instanceof Error && error.message.trim()
    ? error.message
    : fallback;
}

function TicketingStatusStrip({ notices }: { notices: TicketingStatusNotice[] }) {
  if (notices.length === 0) {
    return null;
  }

  return (
    <div
      role="status"
      aria-live="polite"
      className="flex flex-col gap-1 px-4 py-2 text-xs"
      style={{
        backgroundColor: "var(--bg-surface)",
        borderBottomColor: "var(--border-subtle)",
        borderBottomStyle: "solid",
        borderBottomWidth: "1px",
        color: "var(--text-secondary)",
      }}
    >
      {notices.map((notice) => (
        <p key={notice.id} className="flex flex-wrap items-center gap-x-2 gap-y-1">
          <span
            className="font-medium"
            style={{
              color: notice.tone === "error" ? "var(--status-error)" : "var(--status-warning)",
            }}
          >
            {notice.message}
          </span>
          {notice.detail && <span>{notice.detail}</span>}
        </p>
      ))}
    </div>
  );
}

export function TicketingDashboardView({
  projectId,
  onNavigateToAssociation,
}: TicketingDashboardViewProps) {
  const {
    activeProvider,
    activeContainerId,
    viewMode,
    filters,
    selectedTicketRef,
    setProvider,
    setContainerId,
    setViewMode,
    setFilters,
    resetFilters,
    setSelectedTicketRef,
    lastOpenedAt,
    markTicketOpened,
  } = useTicketingStore();
  const setCurrentView = useUiStore((s) => s.setCurrentView);
  const setActiveConversation = useChatStore((s) => s.setActiveConversation);
  const selectAgentConversation = useAgentSessionStore((s) => s.selectConversation);
  const setFocusedAgentProject = useAgentSessionStore((s) => s.setFocusedProject);
  const [startWorkDialogOpen, setStartWorkDialogOpen] = useState(false);
  const [seenBaseline, setSeenBaseline] = useState<string | null>(null);
  const [startWorkSelection, setStartWorkSelection] = useState<StartWorkSelection>({
    projectId,
    mode: "edit",
  });

  const queryClient = useQueryClient();
  const projectsQuery = useProjects();
  const providersQuery = useTicketingProviders(projectId, { enabled: Boolean(projectId) });
  const providers = useMemo(() => providersQuery.data ?? [], [providersQuery.data]);
  const enabledProviders = useMemo(
    () => providers.filter((provider) => provider.enabled),
    [providers],
  );
  const selectableProviders = enabledProviders.length > 0 ? enabledProviders : providers;
  const selectedProvider = providers.find((provider) => provider.provider === activeProvider) ?? null;
  const readableProvider = isProviderReadable(selectedProvider?.connectionStatus);

  useEffect(() => {
    if (selectableProviders.length === 0) {
      return;
    }
    if (
      !activeProvider
      || !selectableProviders.some((provider) => provider.provider === activeProvider)
    ) {
      setProvider(selectableProviders[0]?.provider ?? null);
    }
  }, [activeProvider, selectableProviders, setProvider]);

  const containersQuery = useTicketingContainers(
    activeProvider ? { provider: activeProvider, projectId } : null,
    { enabled: Boolean(activeProvider && readableProvider) },
  );
  const containers = useMemo(() => containersQuery.data ?? [], [containersQuery.data]);

  useEffect(() => {
    if (!activeProvider || containers.length === 0) {
      return;
    }
    if (activeContainerId && containers.some((container) => container.id === activeContainerId)) {
      return;
    }
    setContainerId(containers[0]?.id ?? null);
  }, [activeContainerId, activeProvider, containers, setContainerId]);

  const columnsQuery = useTicketingColumns(
    activeProvider
      ? {
          provider: activeProvider,
          ...(activeContainerId !== null && { containerId: activeContainerId }),
        }
      : null,
    { enabled: Boolean(activeProvider && readableProvider) },
  );
  const columns = columnsQuery.data ?? [];

  const ticketFilters = toTicketFilters(filters);
  const ticketQuery: ListTicketsInput | null = activeProvider && readableProvider
    ? {
        provider: activeProvider,
        projectId,
        limit: 40,
        sort: "updated_desc",
        ...(activeContainerId !== null && { containerId: activeContainerId }),
        ...(ticketFilters !== undefined && { filters: ticketFilters }),
      }
    : null;

  const ticketsQuery = useTickets(ticketQuery, { enabled: Boolean(ticketQuery) });
  const tickets = useMemo(() => flattenTicketPages(ticketsQuery.data), [ticketsQuery.data]);
  const assigneeOptions = useMemo(() => distinctAssigneeNames(tickets), [tickets]);
  const displayedTickets = useMemo(
    () => filterTicketsByAssignee(tickets, filters.assignee),
    [tickets, filters.assignee],
  );
  const ticketColumns = useMemo(() => columnsFromTickets(tickets), [tickets]);
  // Remember the last non-empty columns so the kanban board does not collapse
  // while a refetch briefly returns an empty ticket list.
  const [lastNonEmptyColumns, setLastNonEmptyColumns] = useState<TicketingColumn[]>([]);
  useEffect(() => {
    if (ticketColumns.length > 0) {
      setLastNonEmptyColumns(ticketColumns);
    }
  }, [ticketColumns]);
  const effectiveTicketColumns = ticketColumns.length > 0 ? ticketColumns : lastNonEmptyColumns;
  const statusColumns = effectiveTicketColumns.length > 0
    ? mergeProviderAndTicketColumns(columns, effectiveTicketColumns)
    : columns;
  const selectedSummary = selectedTicketRef
    ? tickets.find((ticket) => ticket.ref.id === selectedTicketRef.id && ticket.ref.provider === selectedTicketRef.provider) ?? null
    : null;
  const shouldHydrateKanban = useAfterPaint(viewMode === "kanban");
  const shouldHydrateDetail = useAfterPaint(selectedTicketRef !== null);
  const detailInput = selectedTicketRef && activeProvider && shouldHydrateDetail
    ? { provider: activeProvider, ticketRef: selectedTicketRef }
    : null;
  const detailQuery = useTicketDetail(detailInput, { enabled: Boolean(detailInput) });
  const transitionsQuery = useTicketTransitions(detailInput, { enabled: Boolean(detailInput) });
  const associationsQuery = useTicketAssociations(
    detailInput ? { ...detailInput, projectId } : null,
    { enabled: Boolean(detailInput && projectId) },
  );
  const refreshTickets = useRefreshTickets();
  const ticketingMutations = useTicketingMutations(projectId);
  const startWorkFromTicket = useStartWorkFromTicket();

  const selectedTicket = detailQuery.data ?? selectedSummary;
  const transitions = useMemo(
    () =>
      transitionsQuery.data
      ?? (detailQuery.data && "transitions" in detailQuery.data ? detailQuery.data.transitions : []),
    [transitionsQuery.data, detailQuery.data],
  );
  const transitionColumns = useMemo(() => columnsFromTransitions(transitions), [transitions]);
  const filterColumns = mergeProviderAndTicketColumns(
    statusColumns,
    transitionColumns,
  );
  const providerName = selectedProvider?.label ?? (activeProvider ? providerLabel(activeProvider) : "Provider");
  const containerLabels = containerLabelsForProvider(activeProvider);
  const statusMessage = selectedProvider?.errorMessage ?? selectedProvider?.permissionMessage ?? undefined;
  const startWorkError = startWorkFromTicket.error instanceof Error
    ? startWorkFromTicket.error.message
    : startWorkFromTicket.error
      ? "RalphX work could not be started."
      : null;
  const statusNotices: TicketingStatusNotice[] = [
    ...(selectedProvider?.staleAt
      ? [{
          id: "stale",
          tone: "warning" as const,
          message: `${providerName} data is stale.`,
          detail: `Last refreshed ${formatRelativeTime(selectedProvider.fetchedAt ?? selectedProvider.staleAt)}.`,
        }]
      : []),
    ...(containersQuery.isError
      ? [{
          id: "containers-error",
          tone: "warning" as const,
          message: "Ticket containers failed to refresh.",
          detail: queryErrorDetail(containersQuery.error, "The current ticket list remains available."),
        }]
      : []),
    ...(columnsQuery.isError
      ? [{
          id: "columns-error",
          tone: "warning" as const,
          message: "Ticket statuses failed to refresh.",
          detail: queryErrorDetail(columnsQuery.error, "Existing ticket rows remain available."),
        }]
      : []),
    ...(ticketsQuery.isError && tickets.length > 0
      ? [{
          id: "tickets-error",
          tone: "warning" as const,
          message: "Tickets failed to refresh.",
          detail: queryErrorDetail(ticketsQuery.error, "Existing ticket rows remain available."),
        }]
      : []),
    ...(refreshTickets.isError
      ? [{
          id: "refresh-error",
          tone: "error" as const,
          message: "Manual refresh failed.",
          detail: queryErrorDetail(refreshTickets.error, "Try again when the provider is available."),
        }]
      : []),
  ];

  const isTicketUnread = useMemo(
    () =>
      (ticket: TicketSummary): boolean =>
        isTicketUpdatedSince(ticket.updatedAt, lastOpenedAt[ticketRefKey(ticket.ref)]),
    [lastOpenedAt],
  );

  function handleSelectTicket(ticket: TicketSummary) {
    const key = ticketRefKey(ticket.ref);
    // Snapshot the prior "seen" timestamp before marking this open so the detail
    // overlay can flag comments that arrived since the last visit.
    setSeenBaseline(lastOpenedAt[key] ?? null);
    setSelectedTicketRef(ticket.ref);
    markTicketOpened(key);
  }

  function handleRefresh() {
    if (!activeProvider) {
      return;
    }
    refreshTickets.mutate({
      provider: activeProvider,
      ...(activeContainerId !== null && { containerId: activeContainerId }),
    });
  }

  async function handleTransitionTicket(transition: TicketTransitionOption) {
    if (!selectedTicket) {
      return;
    }
    await ticketingMutations.transitionStatus({
      provider: selectedTicket.ref.provider,
      ticketRef: selectedTicket.ref,
      transition,
      projectId,
    });
  }

  async function handleAssignToMe() {
    if (!selectedTicket) {
      return;
    }
    await ticketingMutations.assignToMe({
      provider: selectedTicket.ref.provider,
      ticketRef: selectedTicket.ref,
      projectId,
    });
  }

  async function handleClearAssignee() {
    if (!selectedTicket) {
      return;
    }
    await ticketingMutations.clearAssignee({
      provider: selectedTicket.ref.provider,
      ticketRef: selectedTicket.ref,
      projectId,
    });
  }

  function handleQuickAssign(ticket: TicketSummary) {
    void ticketingMutations
      .assignToMe({ provider: ticket.ref.provider, ticketRef: ticket.ref, projectId })
      .catch(() => undefined);
  }

  async function handleAddComment(bodyMarkdown: string) {
    if (!selectedTicket) {
      return;
    }
    await ticketingMutations.addComment({
      provider: selectedTicket.ref.provider,
      ticketRef: selectedTicket.ref,
      bodyMarkdown,
      projectId,
    });
  }

  function handleMoveTicket(ticket: TicketSummary, column: TicketingColumn) {
    const ticketInput = {
      provider: ticket.ref.provider,
      ticketRef: ticket.ref,
    };
    void fetchTicketTransitionsForMove(queryClient, ticketInput)
      .then((ticketTransitions) => {
        const transition = findTicketTransitionForColumn(ticketTransitions, column);
        if (!transition) {
          return undefined;
        }
        return ticketingMutations.transitionStatus({
          ...ticketInput,
          projectId,
          transition,
        });
      })
      .catch(() => undefined);
  }

  useEffect(() => {
    setStartWorkSelection((current) =>
      current.projectId
        ? current
        : { ...current, projectId },
    );
  }, [projectId]);

  function handleStartWorkFromTicket() {
    if (!selectedTicket) {
      return;
    }
    setStartWorkSelection((current) => ({
      ...current,
      projectId: current.projectId || projectId,
    }));
    setStartWorkDialogOpen(true);
  }

  function handleConfirmStartWorkFromTicket() {
    if (!selectedTicket || !startWorkSelection.projectId) {
      return;
    }
    const targetProjectId = startWorkSelection.projectId;
    startWorkFromTicket.mutate({
      projectId: targetProjectId,
      ticketRef: selectedTicket.ref,
      mode: startWorkSelection.mode,
      content: `Start RalphX work for ${ticketKey(selectedTicket.ref)}: ${selectedTicket.title}`,
    }, {
      onSuccess: (result) => {
        const conversationId = result.conversation.id;
        setStartWorkDialogOpen(false);
        setFocusedAgentProject(targetProjectId);
        selectAgentConversation(targetProjectId, conversationId);
        setActiveConversation(`project:${targetProjectId}`, conversationId);
        setCurrentView("agents");
      },
    });
  }

  let content: React.ReactNode;

  if (providersQuery.isLoading) {
    content = (
      <TicketingStatePanel
        state="loading"
        title="Loading ticketing providers"
        description="Provider status and ticket containers are being loaded."
      />
    );
  } else if (providersQuery.isError) {
    content = (
      <TicketingStatePanel
        state="error"
        title="Ticketing providers failed to load"
        description={providersQuery.error instanceof Error ? providersQuery.error.message : "Retry from the toolbar."}
      />
    );
  } else if (providers.length === 0) {
    content = (
      <TicketingStatePanel
        state="empty"
        title="No ticketing providers available"
        description="Connect Jira or Linear from Settings to browse tickets."
      />
    );
  } else if (selectedProvider?.connectionStatus === "disconnected") {
    content = (
      <TicketingStatePanel
        state="disconnected"
        title={`${providerName} is disconnected`}
        description={statusMessage ?? "Reconnect the provider from Settings."}
      />
    );
  } else if (selectedProvider?.connectionStatus === "error") {
    content = (
      <TicketingStatePanel
        state="error"
        title={`${providerName} tickets failed to load`}
        description={statusMessage ?? "Refresh or reconnect the provider from Settings."}
      />
    );
  } else if (selectedProvider?.connectionStatus === "permission_limited") {
    content = (
      <TicketingStatePanel
        state="disconnected"
        title={`${providerName} ticket access is limited`}
        description={statusMessage ?? "Reconnect the provider with ticket search permissions."}
      />
    );
  } else if (ticketsQuery.isLoading || containersQuery.isLoading) {
    content = (
      <TicketingStatePanel
        state="loading"
        title="Loading tickets"
        description="The dashboard shell is ready while ticket data hydrates."
      />
    );
  } else if (ticketsQuery.isError && tickets.length === 0) {
    content = (
      <TicketingStatePanel
        state="error"
        title="Tickets failed to load"
        description={ticketsQuery.error instanceof Error ? ticketsQuery.error.message : "Refresh and try again."}
        actionLabel="Refresh"
        onAction={handleRefresh}
      />
    );
  } else if (displayedTickets.length === 0) {
    content = (
      <TicketingStatePanel
        state="empty"
        title="No tickets match these filters"
        description="Adjust filters or refresh this provider."
        actionLabel="Reset filters"
        onAction={resetFilters}
      />
    );
  } else if (viewMode === "kanban") {
    content = shouldHydrateKanban ? (
      <TicketKanbanView
        columns={statusColumns}
        tickets={displayedTickets}
        canMoveTickets={Boolean(selectedProvider?.capabilities.kanbanWrite)}
        onMoveTicket={handleMoveTicket}
        onSelectTicket={handleSelectTicket}
        isUnread={isTicketUnread}
        canQuickAssign={Boolean(selectedProvider?.capabilities.assignmentWrite)}
        onQuickAssign={handleQuickAssign}
      />
    ) : (
      <TicketKanbanShell columns={statusColumns} />
    );
  } else {
    content = (
      <TicketListView
        tickets={displayedTickets}
        hasNextPage={Boolean(ticketsQuery.hasNextPage)}
        isFetchingNextPage={ticketsQuery.isFetchingNextPage}
        onLoadMore={() => void ticketsQuery.fetchNextPage()}
        onSelectTicket={handleSelectTicket}
        isUnread={isTicketUnread}
        canQuickAssign={Boolean(selectedProvider?.capabilities.assignmentWrite)}
        onQuickAssign={handleQuickAssign}
      />
    );
  }

  return (
    <section
      className="flex h-full min-h-0 flex-col"
      data-testid="ticketing-dashboard"
      data-project-id={projectId}
      style={{
        backgroundColor: "var(--app-content-bg)",
        color: "var(--text-primary)",
      }}
    >
      <header
        className="flex shrink-0 flex-wrap items-center justify-between gap-3 px-4 py-3"
        style={{
          backgroundColor: "var(--bg-surface)",
          borderBottomColor: "var(--border-subtle)",
          borderBottomStyle: "solid",
          borderBottomWidth: "1px",
        }}
      >
        <div className="min-w-0">
          <h1 className="text-lg font-semibold text-[var(--text-primary)]">Ticketing</h1>
          <p className="mt-0.5 text-xs text-[var(--text-muted)]">
            Browse provider tickets and inspect RalphX associations.
          </p>
        </div>
        {enabledProviders.length > 1 && (
          <ProviderSwitcher
            providers={enabledProviders}
            activeProvider={activeProvider}
            onProviderChange={setProvider}
          />
        )}
      </header>

      <TicketFilterBar
        containers={containers}
        columns={filterColumns}
        assigneeOptions={assigneeOptions}
        containerLabel={containerLabels.containerLabel}
        allContainersLabel={containerLabels.allContainersLabel}
        activeContainerId={activeContainerId}
        filters={filters}
        viewMode={viewMode}
        isRefreshing={refreshTickets.isPending || ticketsQuery.isFetching}
        onContainerChange={setContainerId}
        onFiltersChange={setFilters}
        onResetFilters={resetFilters}
        onViewModeChange={setViewMode}
        onRefresh={handleRefresh}
      />

      <TicketingStatusStrip notices={statusNotices} />

      {selectedProvider?.connectionStatus === "permission_limited" && (
        <div
          className="px-4 py-2 text-xs text-[var(--status-warning)]"
          style={{
            backgroundColor: "var(--bg-surface)",
            borderBottomColor: "var(--border-subtle)",
            borderBottomStyle: "solid",
            borderBottomWidth: "1px",
          }}
        >
          {statusMessage ?? `${providerName} has limited permissions. Read-only data may be partial.`}
        </div>
      )}

      <div className="min-h-0 flex-1 overflow-hidden">{content}</div>

      <TicketDetailSheet
        open={selectedTicketRef !== null}
        ticket={selectedTicket}
        capabilities={selectedProvider?.capabilities ?? null}
        transitions={transitions}
        associations={associationsQuery.data}
        isDetailLoading={detailQuery.isLoading}
        isAssociationsLoading={associationsQuery.isLoading}
        isTransitionPending={ticketingMutations.transitionStatusMutation.isPending}
        isAssignPending={ticketingMutations.assignToMeMutation.isPending}
        isCommentPending={ticketingMutations.addCommentMutation.isPending}
        onTransitionTicket={handleTransitionTicket}
        onAssignToMe={handleAssignToMe}
        onClearAssignee={handleClearAssignee}
        onAddComment={handleAddComment}
        seenUntil={seenBaseline}
        isStartWorkPending={startWorkFromTicket.isPending}
        startWorkError={startWorkError}
        onNavigate={onNavigateToAssociation}
        onStartWork={selectedTicket ? handleStartWorkFromTicket : undefined}
        onClose={() => setSelectedTicketRef(null)}
      />

      <StartWorkDialog
        open={startWorkDialogOpen}
        ticket={selectedTicket}
        projects={projectsQuery.data ?? []}
        selected={startWorkSelection}
        isPending={startWorkFromTicket.isPending}
        onSelectionChange={setStartWorkSelection}
        onConfirm={handleConfirmStartWorkFromTicket}
        onClose={() => setStartWorkDialogOpen(false)}
      />
    </section>
  );
}
