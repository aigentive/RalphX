import { useEffect, useMemo } from "react";

import type { ListTicketsInput, TicketDeepLink, TicketFiltersInput, TicketSummary } from "@/api/ticketing";
import {
  flattenTicketPages,
  useRefreshTickets,
  useStartWorkFromTicket,
  useTicketAssociations,
  useTicketDetail,
  useTicketingColumns,
  useTicketingContainers,
  useTicketingProviders,
  useTicketTransitions,
  useTickets,
} from "@/hooks/useTicketing";
import { useTicketingStore } from "@/stores/ticketingStore";

import { ProviderSwitcher } from "./ProviderSwitcher";
import { TicketDetailSheet } from "./TicketDetailSheet";
import { TicketFilterBar } from "./TicketFilterBar";
import { TicketingStatePanel } from "./TicketingStatePanel";
import { TicketKanbanShell, TicketKanbanView, TicketListView } from "./TicketViews";
import { providerLabel, ticketKey } from "./ticketing-utils";
import { useAfterPaint } from "./useAfterPaint";

interface TicketingDashboardViewProps {
  projectId: string;
  onNavigateToAssociation?: ((deepLink: TicketDeepLink) => void) | undefined;
}

function toTicketFilters(filters: ReturnType<typeof useTicketingStore.getState>["filters"]): TicketFiltersInput | undefined {
  const next: TicketFiltersInput = {
    ...(filters.text.trim() && { text: filters.text.trim() }),
    ...(filters.assignee !== null && { assignee: filters.assignee }),
    ...(filters.stateIds.length > 0 && { stateIds: filters.stateIds }),
    ...(filters.labels.length > 0 && { labels: filters.labels }),
  };
  return Object.keys(next).length > 0 ? next : undefined;
}

function isProviderReadable(status: string | undefined): boolean {
  return status === "connected" || status === "permission_limited";
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
  } = useTicketingStore();

  const providersQuery = useTicketingProviders(projectId, { enabled: Boolean(projectId) });
  const providers = useMemo(() => providersQuery.data ?? [], [providersQuery.data]);
  const selectedProvider = providers.find((provider) => provider.provider === activeProvider) ?? null;
  const readableProvider = isProviderReadable(selectedProvider?.connectionStatus);

  useEffect(() => {
    if (providers.length === 0) {
      return;
    }
    if (!activeProvider || !providers.some((provider) => provider.provider === activeProvider)) {
      setProvider(providers[0]?.provider ?? null);
    }
  }, [activeProvider, providers, setProvider]);

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

  const ticketQuery = useMemo<ListTicketsInput | null>(() => {
    if (!activeProvider || !readableProvider) {
      return null;
    }
    const ticketFilters = toTicketFilters(filters);
    return {
      provider: activeProvider,
      projectId,
      limit: 40,
      sort: "updated_desc",
      ...(activeContainerId !== null && { containerId: activeContainerId }),
      ...(ticketFilters !== undefined && { filters: ticketFilters }),
    };
  }, [activeContainerId, activeProvider, filters, projectId, readableProvider]);

  const ticketsQuery = useTickets(ticketQuery, { enabled: Boolean(ticketQuery) });
  const tickets = flattenTicketPages(ticketsQuery.data);
  const selectedSummary = selectedTicketRef
    ? tickets.find((ticket) => ticket.ref.id === selectedTicketRef.id && ticket.ref.provider === selectedTicketRef.provider) ?? null
    : null;
  const shouldHydrateKanban = useAfterPaint(viewMode === "kanban");
  const shouldHydrateDetail = useAfterPaint(selectedTicketRef !== null);
  const detailInput = selectedTicketRef && activeProvider && shouldHydrateDetail
    ? { provider: activeProvider, ticketRef: selectedTicketRef }
    : null;
  const detailQuery = useTicketDetail(detailInput, { enabled: Boolean(detailInput) });
  useTicketTransitions(detailInput, { enabled: Boolean(detailInput) });
  const associationsQuery = useTicketAssociations(
    detailInput ? { ...detailInput, projectId } : null,
    { enabled: Boolean(detailInput && projectId) },
  );
  const refreshTickets = useRefreshTickets();
  const startWorkFromTicket = useStartWorkFromTicket();

  const selectedTicket = detailQuery.data ?? selectedSummary;
  const providerName = selectedProvider?.label ?? (activeProvider ? providerLabel(activeProvider) : "Provider");
  const statusMessage = selectedProvider?.errorMessage ?? selectedProvider?.permissionMessage ?? undefined;
  const startWorkError = startWorkFromTicket.error instanceof Error
    ? startWorkFromTicket.error.message
    : startWorkFromTicket.error
      ? "RalphX work could not be started."
      : null;

  function handleSelectTicket(ticket: TicketSummary) {
    setSelectedTicketRef(ticket.ref);
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

  function handleStartWorkFromTicket() {
    if (!selectedTicket) {
      return;
    }
    startWorkFromTicket.mutate({
      projectId,
      ticketRef: selectedTicket.ref,
      content: `Start RalphX work for ${ticketKey(selectedTicket.ref)}: ${selectedTicket.title}`,
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
  } else if (ticketsQuery.isLoading || containersQuery.isLoading) {
    content = (
      <TicketingStatePanel
        state="loading"
        title="Loading tickets"
        description="The dashboard shell is ready while ticket data hydrates."
      />
    );
  } else if (ticketsQuery.isError) {
    content = (
      <TicketingStatePanel
        state="error"
        title="Tickets failed to load"
        description={ticketsQuery.error instanceof Error ? ticketsQuery.error.message : "Refresh and try again."}
        actionLabel="Refresh"
        onAction={handleRefresh}
      />
    );
  } else if (tickets.length === 0) {
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
        columns={columns}
        tickets={tickets}
        onSelectTicket={handleSelectTicket}
      />
    ) : (
      <TicketKanbanShell columns={columns} />
    );
  } else {
    content = (
      <TicketListView
        tickets={tickets}
        hasNextPage={Boolean(ticketsQuery.hasNextPage)}
        isFetchingNextPage={ticketsQuery.isFetchingNextPage}
        onLoadMore={() => void ticketsQuery.fetchNextPage()}
        onSelectTicket={handleSelectTicket}
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
        <ProviderSwitcher
          providers={providers}
          activeProvider={activeProvider}
          onProviderChange={setProvider}
        />
      </header>

      <TicketFilterBar
        containers={containers}
        columns={columns}
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
        associations={associationsQuery.data}
        isDetailLoading={detailQuery.isLoading}
        isAssociationsLoading={associationsQuery.isLoading}
        isStartWorkPending={startWorkFromTicket.isPending}
        startWorkError={startWorkError}
        onNavigate={onNavigateToAssociation}
        onStartWork={selectedTicket ? handleStartWorkFromTicket : undefined}
        onClose={() => setSelectedTicketRef(null)}
      />
    </section>
  );
}
