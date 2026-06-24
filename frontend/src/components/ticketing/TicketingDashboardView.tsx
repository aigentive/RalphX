import { useEffect, useMemo, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";

import { atlassianApi } from "@/api/atlassian";
import { linearApi } from "@/api/linear";
import type { ComposerIntegrationReference } from "@/api/chat";
import type {
  ListTicketFilterOptionsInput,
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
  useTicketAssociations,
  useTicketDetail,
  useTicketingMutations,
  useTicketingColumns,
  useTicketingContainers,
  useTicketingProviders,
  ticketingKeys,
  useTicketFilterOptions,
  useTicketLabelOptions,
  useTicketTransitions,
  useTickets,
} from "@/hooks/useTicketing";
import { useConversations } from "@/hooks/useChat";
import { useProjects } from "@/hooks/useProjects";
import {
  getValidTicketingProviders,
  isValidTicketingProvider,
} from "@/lib/ticketing-provider-state";
import { useTicketingStore } from "@/stores/ticketingStore";
import { useChatStore } from "@/stores/chatStore";
import { useAgentSessionStore } from "@/stores/agentSessionStore";
import { useUiStore } from "@/stores/uiStore";
import { formatRelativeTime } from "@/lib/formatters";
import { Badge } from "@/components/ui/badge";
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
import { TicketSearchableSelect } from "./TicketSearchableSelect";
import { TicketDetailSheet } from "./TicketDetailSheet";
import { TicketFilterBar } from "./TicketFilterBar";
import { TicketingStatePanel } from "./TicketingStatePanel";
import { TicketKanbanShell, TicketKanbanView, TicketListView } from "./TicketViews";
import {
  distinctAssigneeNames,
  distinctCurrentUserSprintNames,
  filterTicketsByAssignee,
  filterTicketsByProject,
  hasActiveTicketFilters,
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
    ...(filters.watcherMe && { watcherMe: true }),
  };
  return Object.keys(next).length > 0 ? next : undefined;
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
    // Jira containers are projects (read:jira-work), not Agile boards.
    return { containerLabel: "Project", allContainersLabel: "All projects" };
  }
  if (provider === "clickup") {
    // ClickUp containers are Spaces within the selected Workspace (Team).
    return { containerLabel: "Space", allContainersLabel: "All spaces" };
  }
  return { containerLabel: "Container", allContainersLabel: "All containers" };
}

interface StartWorkSelection {
  projectId: string;
}

function StartWorkDialog({
  open,
  ticket,
  projects,
  selected,
  onSelectionChange,
  onConfirm,
  onClose,
}: {
  open: boolean;
  ticket: TicketSummary | null;
  projects: Project[];
  selected: StartWorkSelection;
  onSelectionChange: (selection: StartWorkSelection) => void;
  onConfirm: () => void;
  onClose: () => void;
}) {
  return (
    <Dialog open={open} onOpenChange={(nextOpen) => !nextOpen && onClose()}>
      <DialogContent className="sm:max-w-[460px]">
        <DialogHeader className="block space-y-1.5 px-6 py-5 pr-14">
          <DialogTitle className="text-lg leading-6">Start Conversation</DialogTitle>
          <DialogDescription className="max-w-[34rem] leading-5">
            Choose a project. The new composer will open with{" "}
            {ticket ? ticketKey(ticket.ref) : "this ticket"} attached as a reference.
          </DialogDescription>
        </DialogHeader>

        <div className="grid gap-4 px-6 py-5">
          <label className="grid gap-1.5 text-sm">
            <span className="font-medium text-[var(--text-primary)]">Project</span>
            <TicketSearchableSelect
              ariaLabel="Project"
              size="md"
              value={selected.projectId}
              onValueChange={(projectId) => onSelectionChange({ ...selected, projectId })}
              placeholder={projects.length === 0 ? "No projects" : "Select project"}
              searchPlaceholder="Search projects..."
              emptyLabel="No projects found"
              options={projects.map((project) => ({
                value: project.id,
                label: project.name,
                description: project.workingDirectory ?? undefined,
              }))}
            />
          </label>
        </div>

        <DialogFooter className="px-6 py-4">
          <Button type="button" variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button
            type="button"
            onClick={onConfirm}
            disabled={!selected.projectId || projects.length === 0}
          >
            Open composer
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function ticketComposerReference(ticket: TicketSummary): ComposerIntegrationReference {
  if (ticket.ref.provider === "jira") {
    const id = ticket.ref.key ?? ticket.ref.id;
    return {
      provider: "atlassian",
      kind: "jira",
      id,
      key: id,
      title: ticket.title,
      ...(ticket.url ? { url: ticket.url } : {}),
    };
  }
  if (ticket.ref.provider === "linear") {
    const id = ticket.ref.key ?? ticket.ref.id;
    return {
      provider: "linear",
      kind: "linear",
      id,
      key: id,
      title: ticket.title,
      ...(ticket.url ? { url: ticket.url } : {}),
    };
  }
  const id = ticket.ref.key ?? ticket.ref.id;
  return {
    provider: "clickup",
    kind: "clickup",
    id,
    key: id,
    title: ticket.title,
    ...(ticket.url ? { url: ticket.url } : {}),
  };
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
  const clearAgentSelection = useAgentSessionStore((s) => s.clearSelection);
  const setFocusedAgentProject = useAgentSessionStore((s) => s.setFocusedProject);
  const setStartConversationDraft = useAgentSessionStore((s) => s.setStartConversationDraft);
  const [startWorkDialogOpen, setStartWorkDialogOpen] = useState(false);
  const [seenBaseline, setSeenBaseline] = useState<string | null>(null);
  const [startWorkSelection, setStartWorkSelection] = useState<StartWorkSelection>({
    projectId,
  });

  const queryClient = useQueryClient();
  const projectsQuery = useProjects();
  const providersQuery = useTicketingProviders(projectId, { enabled: Boolean(projectId) });
  const providers = useMemo(() => providersQuery.data ?? [], [providersQuery.data]);
  const validProviders = useMemo(
    () => getValidTicketingProviders(providers),
    [providers],
  );
  const selectedProvider = validProviders.find((provider) => provider.provider === activeProvider) ?? null;
  const readableProvider = isValidTicketingProvider(selectedProvider);

  useEffect(() => {
    if (validProviders.length === 0) {
      if (activeProvider !== null) {
        setProvider(null);
      }
      return;
    }
    if (
      !activeProvider
      || !validProviders.some((provider) => provider.provider === activeProvider)
    ) {
      setProvider(validProviders[0]?.provider ?? null);
    }
  }, [activeProvider, validProviders, setProvider]);

  const containersQuery = useTicketingContainers(
    activeProvider ? { provider: activeProvider, projectId } : null,
    { enabled: Boolean(activeProvider && readableProvider) },
  );
  const containers = useMemo(() => containersQuery.data ?? [], [containersQuery.data]);

  // When Linear is the only enabled provider, auto-load its tickets instead of
  // forcing a container pick. Jira projects and ClickUp Spaces remain explicit
  // scopes so the dashboard does not fire broad provider-wide fetches.
  const autoLoadsWithoutContainer =
    validProviders.length === 1 &&
    activeProvider === "linear";
  // When a readable provider exposes containers but none is selected, force the
  // user to pick one (no auto-select) and gate the columns/tickets queries so no
  // provider-wide unfiltered fetch fires.
  const requiresContainer =
    readableProvider && containers.length > 0 && !autoLoadsWithoutContainer;
  const containerSelectionNeeded = requiresContainer && activeContainerId === null;

  useEffect(() => {
    if (!activeProvider || containers.length === 0) {
      return;
    }
    // Default to "All projects" (null); only clear a now-stale selection so the
    // user opts into a specific container rather than being forced into the first.
    if (activeContainerId && !containers.some((container) => container.id === activeContainerId)) {
      setContainerId(null);
    }
  }, [activeContainerId, activeProvider, containers, setContainerId]);

  const columnsQuery = useTicketingColumns(
    activeProvider && !containerSelectionNeeded
      ? {
          provider: activeProvider,
          ...(activeContainerId !== null && { containerId: activeContainerId }),
        }
      : null,
    { enabled: Boolean(activeProvider && readableProvider && !containerSelectionNeeded) },
  );
  const columns = columnsQuery.data ?? [];

  const ticketFilters = toTicketFilters(filters);
  const ticketQuery: ListTicketsInput | null = activeProvider && readableProvider && !containerSelectionNeeded
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
  const filterOptionsInput: ListTicketFilterOptionsInput | null = activeProvider && readableProvider && !containerSelectionNeeded
    ? {
        provider: activeProvider,
        projectId,
        limit: 500,
        ...(activeContainerId !== null && { containerId: activeContainerId }),
        ...(ticketFilters !== undefined && { filters: ticketFilters }),
      }
    : null;
  const filterOptionsQuery = useTicketFilterOptions(filterOptionsInput, {
    enabled: Boolean(filterOptionsInput),
  });
  const hasWatcherMetadata = useMemo(
    () =>
      tickets.some((ticket) =>
        ticket.currentUserWatching || (ticket.watchers?.length ?? 0) > 0,
      ),
    [tickets],
  );
  const pageAssigneeOptions = useMemo(() => distinctAssigneeNames(tickets), [tickets]);
  const pageSprintOptions = useMemo(
    () => (activeProvider === "clickup" ? distinctCurrentUserSprintNames(tickets) : []),
    [activeProvider, tickets],
  );
  const assigneeOptions = filterOptionsQuery.data?.assignees ?? pageAssigneeOptions;
  const sprintOptions = useMemo(
    () =>
      activeProvider === "clickup" ? filterOptionsQuery.data?.sprints ?? pageSprintOptions : [],
    [activeProvider, filterOptionsQuery.data?.sprints, pageSprintOptions],
  );
  useEffect(() => {
    if (!filters.sprint) {
      return;
    }
    if (activeProvider !== "clickup" || !sprintOptions.includes(filters.sprint)) {
      setFilters({ sprint: null });
    }
  }, [activeProvider, filters.sprint, setFilters, sprintOptions]);
  useEffect(() => {
    if (activeProvider !== "clickup" && filters.watcherMe) {
      setFilters({ watcherMe: false });
    }
  }, [activeProvider, filters.watcherMe, setFilters]);
  const activeContainerName = useMemo(
    () => containers.find((container) => container.id === activeContainerId)?.name ?? null,
    [containers, activeContainerId],
  );
  // Jira and ClickUp scope tickets to the selected container server-side (issues
  // carry no matching `project` field), so the client-side container-name filter
  // must be skipped for them; Linear returns all issues and relies on this
  // client filter.
  const clientContainerFilter =
    activeProvider === "jira" || activeProvider === "clickup" ? null : activeContainerName;
  const displayedTickets = useMemo(
    () =>
      filterTicketsByProject(
        filterTicketsByProject(
          filterTicketsByAssignee(tickets, filters.assignee),
          activeProvider === "clickup" ? filters.sprint : null,
        ),
        clientContainerFilter,
      ),
    [tickets, filters.assignee, filters.sprint, activeProvider, clientContainerFilter],
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
  // When a board-supporting provider has no container selected, show no statuses
  // at all (the remembered last-non-empty columns must not leak a prior project's
  // statuses into the filter/board until a project is chosen).
  const statusColumns = containerSelectionNeeded
    ? []
    : effectiveTicketColumns.length > 0
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
  // Linear pick-list needs the issue team's labels; Jira is free-text and skips it.
  const labelOptionsQuery = useTicketLabelOptions(detailInput, {
    enabled: Boolean(detailInput) && activeProvider === "linear",
  });
  const associationsQuery = useTicketAssociations(
    detailInput ? { ...detailInput, projectId } : null,
    { enabled: Boolean(detailInput && projectId) },
  );
  const refreshTickets = useRefreshTickets();
  const ticketingMutations = useTicketingMutations(projectId);
  const conversationsQuery = useConversations({ view: "ticketing", projectId });
  const bindableConversations = useMemo(
    () =>
      (conversationsQuery.data ?? []).map((conversation) => ({
        id: conversation.id,
        title: conversation.title,
      })),
    [conversationsQuery.data],
  );
  const bindConversation = useMutation({
    mutationFn: async (conversationId: string) => {
      if (!selectedTicket) {
        throw new Error("No ticket selected to bind a conversation to.");
      }
      const ticketRef = selectedTicket.ref;
      if (ticketRef.provider === "jira") {
        await atlassianApi.assignAgentConversationJiraIssue({
          conversationId,
          projectId,
          issueKey: ticketRef.key ?? ticketRef.id,
          issueId: ticketRef.id,
          ...(selectedTicket.title !== undefined && { title: selectedTicket.title }),
          ...(selectedTicket.url !== undefined && { issueUrl: selectedTicket.url }),
          refresh: true,
        });
      } else {
        await linearApi.assignAgentConversationLinearIssue({
          conversationId,
          projectId,
          issueId: ticketRef.id,
          ...(ticketRef.key !== undefined && { issueKey: ticketRef.key }),
          ...(selectedTicket.title !== undefined && { title: selectedTicket.title }),
          ...(selectedTicket.url !== undefined && { issueUrl: selectedTicket.url }),
          refresh: true,
        });
      }
      return conversationId;
    },
    onSuccess: (conversationId) => {
      if (selectedTicket) {
        void queryClient.invalidateQueries({
          queryKey: ticketingKeys.associations({
            provider: selectedTicket.ref.provider,
            ticketRef: selectedTicket.ref,
            projectId,
          }),
        });
      }
      void queryClient.invalidateQueries({
        queryKey: ticketingKeys.conversationTicket(conversationId),
      });
      // Refresh the ticket lists so the RX (association count) column activates for
      // the ticket whose conversation was just bound.
      void queryClient.invalidateQueries({ queryKey: ticketingKeys.ticketLists() });
    },
  });
  const bindError = bindConversation.error instanceof Error
    ? bindConversation.error.message
    : bindConversation.error
      ? "Conversation could not be bound."
      : null;

  function handleBindConversation(conversationId: string) {
    bindConversation.mutate(conversationId);
  }

  // Only treat the loaded detail as the selected ticket when it actually matches
  // the current selection, so switching tickets never flashes the previous one.
  const detailMatchesSelection = Boolean(
    detailQuery.data
    && selectedTicketRef
    && detailQuery.data.ref.id === selectedTicketRef.id
    && detailQuery.data.ref.provider === selectedTicketRef.provider,
  );
  const selectedTicket = (detailMatchesSelection ? detailQuery.data : selectedSummary) ?? null;
  // Show the overlay preloader until the matching full detail is ready.
  const isDetailPending = selectedTicketRef !== null && !detailMatchesSelection;
  const transitions = useMemo(
    () =>
      transitionsQuery.data
      ?? (detailQuery.data && "transitions" in detailQuery.data ? detailQuery.data.transitions : []),
    [transitionsQuery.data, detailQuery.data],
  );
  const transitionColumns = useMemo(() => columnsFromTransitions(transitions), [transitions]);
  const filterColumns = containerSelectionNeeded
    ? []
    : mergeProviderAndTicketColumns(statusColumns, transitionColumns);
  const providerName = selectedProvider?.label ?? (activeProvider ? providerLabel(activeProvider) : "Provider");
  const containerLabels = containerLabelsForProvider(activeProvider);
  // ClickUp conversation-linking is deferred (no ClickUp link table yet), so
  // binding an existing conversation stays hidden. Starting new RalphX work is
  // still provider-neutral and includes the ticket reference in the composer.
  const supportsConversationBinding = activeProvider !== "clickup";
  const statusMessage = selectedProvider?.errorMessage ?? selectedProvider?.permissionMessage ?? undefined;
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
    ...(filterOptionsQuery.isError
      ? [{
          id: "filter-options-error",
          tone: "warning" as const,
          message: "Ticket filters failed to refresh.",
          detail: queryErrorDetail(filterOptionsQuery.error, "Options from the current ticket page remain available."),
        }]
      : []),
    ...(filterOptionsQuery.data?.truncated
      ? [{
          id: "filter-options-truncated",
          tone: "warning" as const,
          message: "Ticket filter options are truncated.",
          detail: "Search or narrow the ticket scope if an option is missing.",
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

  async function handleSetLabels(labels: string[]) {
    if (!selectedTicket) {
      return;
    }
    await ticketingMutations.setLabels({
      provider: selectedTicket.ref.provider,
      ticketRef: selectedTicket.ref,
      labels,
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
    setStartConversationDraft({
      projectId: targetProjectId,
      content: "",
      mode: "edit",
      composerIntegrationReferences: [ticketComposerReference(selectedTicket)],
    });
    setStartWorkDialogOpen(false);
    setFocusedAgentProject(targetProjectId);
    clearAgentSelection();
    setActiveConversation(`project:${targetProjectId}`, null);
    setCurrentView("agents");
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
        description="Connect Jira, Linear, or ClickUp from Settings to browse tickets."
      />
    );
  } else if (validProviders.length === 0) {
    content = (
      <TicketingStatePanel
        state="disconnected"
        title="No valid ticketing integration"
        description="Connect a valid Jira or Linear integration from Settings to browse tickets."
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
  } else if (containerSelectionNeeded) {
    const containerNoun = containerLabels.containerLabel.toLowerCase();
    content = (
      <TicketingStatePanel
        state="empty"
        title={`Select a ${containerNoun}`}
        description={`Choose a ${containerNoun} to load its tickets and statuses.`}
      />
    );
  } else if (displayedTickets.length === 0) {
    content = hasActiveTicketFilters(filters) ? (
      <TicketingStatePanel
        state="empty"
        title="No tickets match these filters"
        description="Adjust filters or refresh this provider."
        actionLabel="Reset filters"
        onAction={resetFilters}
      />
    ) : (
      <TicketingStatePanel
        state="empty"
        title="No tickets here yet"
        description="Refresh this provider or start RalphX work from a ticket."
        actionLabel="Refresh"
        onAction={handleRefresh}
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
        columns={statusColumns}
        hasNextPage={Boolean(ticketsQuery.hasNextPage)}
        isFetchingNextPage={ticketsQuery.isFetchingNextPage}
        onLoadMore={() => void ticketsQuery.fetchNextPage()}
        onSelectTicket={handleSelectTicket}
        isUnread={isTicketUnread}
        canQuickAssign={Boolean(selectedProvider?.capabilities.assignmentWrite)}
        onQuickAssign={handleQuickAssign}
        canMoveTickets={Boolean(selectedProvider?.capabilities.kanbanWrite)}
        onMoveTicket={handleMoveTicket}
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
          <h1
            aria-label="Ticketing"
            className="flex min-w-0 items-center gap-2 text-lg font-semibold text-[var(--text-primary)]"
          >
            <span>Ticketing</span>
            <Badge
              data-testid="ticketing-visible-count"
              aria-label={`${displayedTickets.length} visible ${displayedTickets.length === 1 ? "ticket" : "tickets"}`}
              variant="outline"
              className="h-5 rounded-full border-[var(--border-subtle)] bg-[var(--bg-elevated)] px-2 text-xs font-medium leading-none text-[var(--text-muted)]"
            >
              {displayedTickets.length}
            </Badge>
          </h1>
          <p className="mt-0.5 text-xs text-[var(--text-muted)]">
            Browse provider tickets and inspect RalphX associations.
          </p>
        </div>
        {validProviders.length > 1 && (
          <ProviderSwitcher
            providers={validProviders}
            activeProvider={activeProvider}
            onProviderChange={setProvider}
          />
        )}
      </header>

      <TicketFilterBar
        containers={containers}
        columns={filterColumns}
        assigneeOptions={assigneeOptions}
        sprintOptions={sprintOptions}
        showWatcherFilter={
          activeProvider === "clickup" && (hasWatcherMetadata || filters.watcherMe)
        }
        containerLabel={containerLabels.containerLabel}
        allContainersLabel={containerLabels.allContainersLabel}
        activeContainerId={activeContainerId}
        containerSelectionNeeded={containerSelectionNeeded}
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

      <div className="flex min-h-0 flex-1 overflow-hidden">{content}</div>

      <TicketDetailSheet
        open={selectedTicketRef !== null}
        ticket={selectedTicket}
        capabilities={selectedProvider?.capabilities ?? null}
        transitions={transitions}
        associations={associationsQuery.data}
        isDetailLoading={isDetailPending}
        isAssociationsLoading={associationsQuery.isLoading}
        isTransitionPending={ticketingMutations.transitionStatusMutation.isPending}
        isAssignPending={ticketingMutations.assignToMeMutation.isPending}
        isCommentPending={ticketingMutations.addCommentMutation.isPending}
        isLabelPending={ticketingMutations.setLabelsMutation.isPending}
        labelOptions={labelOptionsQuery.data}
        isLabelOptionsLoading={labelOptionsQuery.isLoading}
        onTransitionTicket={handleTransitionTicket}
        onAssignToMe={handleAssignToMe}
        onClearAssignee={handleClearAssignee}
        onAddComment={handleAddComment}
        onSetLabels={selectedTicket ? handleSetLabels : undefined}
        seenUntil={seenBaseline}
        isStartWorkPending={false}
        startWorkError={null}
        showConversationBinding={supportsConversationBinding}
        bindableConversations={bindableConversations}
        onBindConversation={selectedTicket && supportsConversationBinding ? handleBindConversation : undefined}
        isBindPending={bindConversation.isPending}
        bindError={bindError}
        onNavigate={onNavigateToAssociation}
        onStartWork={selectedTicket ? handleStartWorkFromTicket : undefined}
        onClose={() => setSelectedTicketRef(null)}
      />

      <StartWorkDialog
        open={startWorkDialogOpen}
        ticket={selectedTicket}
        projects={projectsQuery.data ?? []}
        selected={startWorkSelection}
        onSelectionChange={setStartWorkSelection}
        onConfirm={handleConfirmStartWorkFromTicket}
        onClose={() => setStartWorkDialogOpen(false)}
      />
    </section>
  );
}
