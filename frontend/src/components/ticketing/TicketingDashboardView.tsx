import { useEffect, useMemo, useRef, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { ArrowDown, ArrowUp, ListOrdered, RefreshCw, RotateCcw } from "lucide-react";
import { toast } from "sonner";

import { atlassianApi } from "@/api/atlassian";
import { linearApi } from "@/api/linear";
import type { ComposerIntegrationReference } from "@/api/chat";
import type {
  ListTicketFilterOptionsInput,
  ListTicketsInput,
  TicketDeepLink,
  TicketFiltersInput,
  TicketRef,
  TicketingColumn,
  TicketingContainer,
  TicketingProvider,
  TicketingStatusCatalogEntry,
  TicketingStatusCatalogScopeInput,
  TicketSummary,
  TicketTransitionOption,
} from "@/api/ticketing";
import type { Project } from "@/types/project";
import {
  fetchTicketTransitionsForMove,
  findTicketTransitionForColumn,
  flattenTicketPages,
  useRefreshTickets,
  useRefreshTicketingStatusCatalog,
  useTicketAssociations,
  useTicketDetail,
  useTicketingMutations,
  useTicketingColumns,
  useTicketingContainers,
  useTicketingProviders,
  ticketingKeys,
  useTicketingStatusCatalog,
  useTicketFilterOptions,
  useTicketLabelOptions,
  useTicketTransitions,
  useTickets,
  useUpdateTicketingStatusPresentation,
} from "@/hooks/useTicketing";
import { useConversations } from "@/hooks/useChat";
import { useAgentGate } from "@/hooks/useAgentGate";
import { useProjects } from "@/hooks/useProjects";
import {
  getValidTicketingProviders,
  isValidTicketingProvider,
} from "@/lib/ticketing-provider-state";
import { cn } from "@/lib/utils";
import { useTicketingStore } from "@/stores/ticketingStore";
import { useChatStore } from "@/stores/chatStore";
import { useAgentSessionStore } from "@/stores/agentSessionStore";
import { useUiStore } from "@/stores/uiStore";
import { PullRequestDetailSheet } from "@/components/pr/PullRequestDetailSheet";
import {
  pullRequestSelectorFromShell,
  pullRequestShellFromTicket,
  type PullRequestShell,
} from "@/components/pr/PullRequestDetailShell";
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
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";

import { ProviderSwitcher } from "./ProviderSwitcher";
import { TicketSearchableSelect } from "./TicketSearchableSelect";
import { TicketDetailSheet } from "./TicketDetailSheet";
import { TicketFilterBar } from "./TicketFilterBar";
import { TicketStatusGlyph } from "./TicketStatusGlyph";
import { TicketingStatePanel } from "./TicketingStatePanel";
import { TicketKanbanShell, TicketKanbanView, TicketListView } from "./TicketViews";
import {
  distinctAssigneeNames,
  distinctCurrentUserSprintNames,
  filterTicketsByProject,
  hasActiveTicketFilters,
  isTicketUpdatedSince,
  ticketRefKey,
  UNASSIGNED_ASSIGNEE,
} from "./ticketing-read-state";
import {
  mergeProviderAndTicketColumns,
  normalizeStatusKey,
} from "./ticketing-status-presentation";
import { providerLabel, ticketKey } from "./ticketing-utils";
import { useAfterPaint } from "./useAfterPaint";

interface TicketingDashboardViewProps {
  projectId: string;
  onNavigateToAssociation?: ((deepLink: TicketDeepLink) => void) | undefined;
}

function toTicketFilters(filters: ReturnType<typeof useTicketingStore.getState>["filters"]): TicketFiltersInput | undefined {
  const next: TicketFiltersInput = {
    ...(filters.text.trim() && { text: filters.text.trim() }),
    ...(filters.assignees.length > 0 && { assignees: filters.assignees }),
    ...(filters.stateIds.length > 0 && { stateIds: filters.stateIds }),
    ...(filters.labels.length > 0 && { labels: filters.labels }),
    ...(filters.sprint && { sprint: filters.sprint }),
    ...(filters.watcherMe && { watcherMe: true }),
  };
  return Object.keys(next).length > 0 ? next : undefined;
}

function toTicketFilterOptionFilters(
  filters: ReturnType<typeof useTicketingStore.getState>["filters"],
): TicketFiltersInput | undefined {
  const text = filters.text.trim();
  return text ? { text } : undefined;
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
    return { containerLabel: "Space", allContainersLabel: "Select space" };
  }
  return { containerLabel: "Container", allContainersLabel: "All containers" };
}

function clickupSpaceForContainer(
  containerId: string | null,
  containers: TicketingContainer[],
): string | null {
  if (!containerId) {
    return null;
  }
  const byId = new Map(containers.map((container) => [container.id, container]));
  let current = byId.get(containerId) ?? null;
  while (current) {
    if (current.kind === "space") {
      return current.id;
    }
    current = current.parentId ? byId.get(current.parentId) ?? null : null;
  }
  return null;
}

function clickupChildLocations(
  spaceId: string | null,
  containers: TicketingContainer[],
): TicketingContainer[] {
  if (!spaceId) {
    return [];
  }
  const folderIds = new Set(
    containers
      .filter((container) => container.kind === "folder" && container.parentId === spaceId)
      .map((container) => container.id),
  );
  return containers.filter((container) => (
    (container.kind === "folder" && container.parentId === spaceId)
    || (container.kind === "list" && (container.parentId === spaceId || folderIds.has(container.parentId ?? "")))
  ));
}

function statusCatalogScopeForSelection(
  provider: TicketingProvider | null,
  containerId: string | null,
  container: TicketingContainer | null,
): TicketingStatusCatalogScopeInput | null {
  if (!provider) {
    return null;
  }
  if (provider === "jira") {
    return containerId
      ? { provider, scopeKind: "jira_project", scopeId: containerId }
      : null;
  }
  if (provider === "clickup") {
    if (!containerId) {
      return null;
    }
    const kind = container?.kind ?? (
      containerId.startsWith("folder:")
        ? "folder"
        : containerId.startsWith("list:")
          ? "list"
          : "space"
    );
    if (kind !== "space" && kind !== "folder" && kind !== "list") {
      return null;
    }
    return {
      provider,
      scopeKind: `clickup_${kind}`,
      scopeId: containerId.replace(/^(space|folder|list):/, ""),
    };
  }
  return { provider, scopeKind: "linear_global", scopeId: "all" };
}

function statusCatalogScopeKey(scope: TicketingStatusCatalogScopeInput | null): string | null {
  return scope ? `${scope.provider}:${scope.scopeKind}:${scope.scopeId}` : null;
}

function statusCatalogScopeLabel(
  provider: TicketingProvider | null,
  scope: TicketingStatusCatalogScopeInput | null,
  activeContainerName: string | null,
): string {
  if (!provider || !scope) {
    return "No scope selected";
  }
  if (provider === "jira") {
    return activeContainerName ?? scope.scopeId;
  }
  if (provider === "clickup") {
    return activeContainerName ?? scope.scopeId;
  }
  return "All Linear workflow states";
}

function catalogEntryColor(entry: TicketingStatusCatalogEntry): string {
  return entry.color?.trim() || "var(--text-muted)";
}

function colorInputValue(entry: TicketingStatusCatalogEntry): string {
  const color = entry.colorOverride?.trim() || entry.providerColor?.trim();
  return color && /^#[0-9a-f]{6}$/i.test(color) ? color : "#6b7280";
}

function sortedStatusCatalogEntries(
  entries: TicketingStatusCatalogEntry[],
): TicketingStatusCatalogEntry[] {
  return [...entries].sort((left, right) =>
    left.displayOrder - right.displayOrder
    || (left.providerOrder ?? Number.MAX_SAFE_INTEGER) - (right.providerOrder ?? Number.MAX_SAFE_INTEGER)
    || left.providerStatusName.localeCompare(right.providerStatusName),
  );
}

function ticketStatusCountsByKey(tickets: TicketSummary[]): Map<string, number> {
  const counts = new Map<string, number>();
  for (const ticket of tickets) {
    const keys = new Set(
      [normalizeStatusKey(ticket.state.id), normalizeStatusKey(ticket.state.name)]
        .filter((value): value is string => Boolean(value)),
    );
    for (const key of keys) {
      counts.set(key, (counts.get(key) ?? 0) + 1);
    }
  }
  return counts;
}

function catalogEntryTicketCount(
  entry: TicketingStatusCatalogEntry,
  counts: Map<string, number> | null | undefined,
): number {
  if (!counts) {
    return 0;
  }
  const keys = [
    normalizeStatusKey(entry.providerStatusId),
    normalizeStatusKey(entry.providerStatusName),
  ].filter((value): value is string => Boolean(value));
  return Math.max(0, ...keys.map((key) => counts.get(key) ?? 0));
}

function ClickUpLocationRail({
  space,
  locations,
  activeContainerId,
  onSelect,
}: {
  space: TicketingContainer;
  locations: TicketingContainer[];
  activeContainerId: string | null;
  onSelect: (containerId: string) => void;
}) {
  const firstLevelLocations = locations.filter((location) => location.parentId === space.id);

  const itemClass = (selected: boolean) =>
    cn(
      "flex h-7 w-full items-center rounded px-2 text-left text-xs",
      "focus-visible:outline-none focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:1px]",
      selected ? "font-medium text-[var(--accent-primary)]" : "text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]",
    );

  return (
    <aside
      className="w-56 shrink-0 overflow-y-auto border-r px-3 py-3"
      style={{
        backgroundColor: "var(--bg-surface)",
        borderColor: "var(--border-subtle)",
      }}
      aria-label="ClickUp locations"
    >
      <button
        type="button"
        className={itemClass(activeContainerId === space.id)}
        onClick={() => onSelect(space.id)}
      >
        All in {space.name}
      </button>
      <div className="mt-2 space-y-0.5">
        {firstLevelLocations.map((location) => (
          <button
            key={location.id}
            type="button"
            className={itemClass(activeContainerId === location.id)}
            onClick={() => onSelect(location.id)}
          >
            {location.name}
          </button>
        ))}
      </div>
    </aside>
  );
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
  return {
    provider: "clickup",
    kind: "clickup",
    id: ticket.ref.id,
    key: ticket.ref.key ?? ticket.ref.id,
    title: ticket.title,
    ...(ticket.url ? { url: ticket.url } : {}),
  };
}

function ticketRefsIdentifySameTicket(left: TicketRef, right: TicketRef): boolean {
  if (left.provider !== right.provider) {
    return false;
  }
  const leftIds = ticketRefAliases(left);
  const rightIds = ticketRefAliases(right);
  return leftIds.some((leftId) => rightIds.includes(leftId));
}

function ticketRefAliases(ref: TicketRef): string[] {
  return ref.key ? [ref.id, ref.key] : [ref.id];
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

function StatusManagementDialog({
  open,
  providerName,
  scopeLabel,
  entries,
  ticketCounts = new Map<string, number>(),
  isLoading,
  isSyncing,
  isUpdating,
  error,
  onSync,
  onMove,
  onColorChange,
  onResetColor,
  onVisibilityChange,
  onClose,
}: {
  open: boolean;
  providerName: string;
  scopeLabel: string;
  entries: TicketingStatusCatalogEntry[];
  ticketCounts?: Map<string, number> | null;
  isLoading: boolean;
  isSyncing: boolean;
  isUpdating: boolean;
  error: string | null;
  onSync: () => void;
  onMove: (entry: TicketingStatusCatalogEntry, direction: -1 | 1) => void;
  onColorChange: (entry: TicketingStatusCatalogEntry, color: string) => void;
  onResetColor: (entry: TicketingStatusCatalogEntry) => void;
  onVisibilityChange: (entry: TicketingStatusCatalogEntry, visible: boolean) => void;
  onClose: () => void;
}) {
  const sortedEntries = sortedStatusCatalogEntries(entries);
  const controlsDisabled = isSyncing || isUpdating;

  return (
    <Dialog open={open} onOpenChange={(nextOpen) => !nextOpen && onClose()}>
      <DialogContent className="max-h-[82vh] overflow-hidden sm:max-w-[760px]">
        <DialogHeader className="block space-y-1.5 px-6 py-5 pr-14">
          <DialogTitle className="text-lg leading-6">Statuses</DialogTitle>
          <DialogDescription className="leading-5">
            {providerName} · {scopeLabel}
          </DialogDescription>
        </DialogHeader>

        <div className="min-h-0 overflow-y-auto px-6 py-4">
          <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
            <div className="text-xs text-[var(--text-muted)]">
              {isSyncing ? "Syncing statuses" : `${sortedEntries.length} statuses`}
            </div>
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={onSync}
              disabled={controlsDisabled}
            >
              <RefreshCw className={isSyncing ? "animate-spin" : ""} aria-hidden="true" />
              Sync
            </Button>
          </div>

          {error && (
            <div
              role="status"
              className="mb-3 rounded-md px-3 py-2 text-xs"
              style={{
                backgroundColor: "var(--bg-surface)",
                borderColor: "var(--status-error)",
                borderStyle: "solid",
                borderWidth: "1px",
                color: "var(--status-error)",
              }}
            >
              {error}
            </div>
          )}

          {isLoading && sortedEntries.length === 0 ? (
            <div className="py-10 text-center text-sm text-[var(--text-secondary)]">
              Loading statuses
            </div>
          ) : sortedEntries.length === 0 ? (
            <div className="py-10 text-center text-sm text-[var(--text-secondary)]">
              No statuses synced for this scope
            </div>
          ) : (
            <div
              className="overflow-hidden rounded-md"
              style={{
                borderColor: "var(--border-subtle)",
                borderStyle: "solid",
                borderWidth: "1px",
              }}
            >
              <div
                className="grid grid-cols-[76px_minmax(0,1fr)_132px_160px_88px] items-center gap-3 px-3 py-2 text-[11px] font-medium uppercase text-[var(--text-muted)]"
                style={{
                  backgroundColor: "var(--bg-surface)",
                  borderBottomColor: "var(--border-subtle)",
                  borderBottomStyle: "solid",
                  borderBottomWidth: "1px",
                }}
              >
                <span>Order</span>
                <span>Status</span>
                <span>Provider</span>
                <span>Color</span>
                <span>Visible</span>
              </div>
              {sortedEntries.map((entry, index) => {
                const ticketCount = catalogEntryTicketCount(entry, ticketCounts);
                const ticketCountLabel = `${ticketCount} ${ticketCount === 1 ? "ticket" : "tickets"} in ${entry.providerStatusName}`;
                return (
                  <div
                    key={entry.providerStatusId}
                    className="grid grid-cols-[76px_minmax(0,1fr)_132px_160px_88px] items-center gap-3 px-3 py-2 text-sm"
                    style={{
                      backgroundColor: "var(--bg-elevated)",
                      borderBottomColor: index === sortedEntries.length - 1
                        ? "transparent"
                        : "var(--border-subtle)",
                      borderBottomStyle: "solid",
                      borderBottomWidth: "1px",
                    }}
                  >
                    <div className="flex gap-1">
                      <Tooltip>
                        <TooltipTrigger asChild>
                          <Button
                            type="button"
                            variant="ghost"
                            size="icon-sm"
                            className="h-7 w-7"
                            aria-label={`Move ${entry.providerStatusName} up`}
                            disabled={controlsDisabled || index === 0}
                            onClick={() => onMove(entry, -1)}
                          >
                            <ArrowUp aria-hidden="true" />
                          </Button>
                        </TooltipTrigger>
                        <TooltipContent>Move up</TooltipContent>
                      </Tooltip>
                      <Tooltip>
                        <TooltipTrigger asChild>
                          <Button
                            type="button"
                            variant="ghost"
                            size="icon-sm"
                            className="h-7 w-7"
                            aria-label={`Move ${entry.providerStatusName} down`}
                            disabled={controlsDisabled || index === sortedEntries.length - 1}
                            onClick={() => onMove(entry, 1)}
                          >
                            <ArrowDown aria-hidden="true" />
                          </Button>
                        </TooltipTrigger>
                        <TooltipContent>Move down</TooltipContent>
                      </Tooltip>
                    </div>
                    <div className="flex min-w-0 items-center gap-2">
                      <span
                        className="inline-flex shrink-0"
                        aria-hidden="true"
                        style={{ color: catalogEntryColor(entry) }}
                      >
                        <TicketStatusGlyph category={entry.providerCategory} className="h-4 w-4" />
                      </span>
                      <div className="min-w-0" title={entry.providerStatusId}>
                        <div className="flex min-w-0 items-center gap-2">
                          <span className="truncate font-medium text-[var(--text-primary)]">
                            {entry.providerStatusName}
                          </span>
                          <Badge
                            variant="outline"
                            aria-label={ticketCountLabel}
                            className="h-5 shrink-0 rounded-full border-[var(--border-subtle)] px-2 text-[10px] font-medium text-[var(--text-muted)]"
                          >
                            {ticketCount}
                          </Badge>
                        </div>
                      </div>
                    </div>
                    <div className="flex min-w-0 items-center gap-2 text-xs text-[var(--text-secondary)]">
                      <span className="truncate">{entry.providerCategory.replace(/_/g, " ")}</span>
                      {entry.stale && (
                        <Badge variant="outline" className="h-5 rounded-full px-2 text-[10px]">
                          Stale
                        </Badge>
                      )}
                    </div>
                    <div className="flex items-center gap-2">
                      <Input
                        aria-label={`Color for ${entry.providerStatusName}`}
                        type="color"
                        className="h-8 w-10 shrink-0 cursor-pointer p-1"
                        value={colorInputValue(entry)}
                        disabled={controlsDisabled}
                        onChange={(event) => onColorChange(entry, event.currentTarget.value)}
                      />
                      <Button
                        type="button"
                        variant="ghost"
                        size="sm"
                        className="h-7 px-2"
                        disabled={controlsDisabled || !entry.colorOverride}
                        onClick={() => onResetColor(entry)}
                      >
                        <RotateCcw aria-hidden="true" />
                        Reset
                      </Button>
                    </div>
                    <div className="flex justify-end">
                      <Switch
                        aria-label={`Show ${entry.providerStatusName}`}
                        checked={entry.isVisible}
                        disabled={controlsDisabled}
                        onCheckedChange={(checked) => onVisibilityChange(entry, checked)}
                      />
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </div>

        <DialogFooter className="px-6 py-4">
          <Button type="button" variant="ghost" onClick={onClose}>
            Close
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
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
  const [selectedPullRequestShell, setSelectedPullRequestShell] =
    useState<PullRequestShell | null>(null);
  const [statusManagerOpen, setStatusManagerOpen] = useState(false);
  const [seenBaseline, setSeenBaseline] = useState<string | null>(null);
  const [startWorkSelection, setStartWorkSelection] = useState<StartWorkSelection>({
    projectId,
  });
  const lastAutoSyncedStatusScope = useRef<string | null>(null);

  const queryClient = useQueryClient();
  const providersReadGate = useAgentGate("ticketingProvidersRead");
  const statusCatalogReadGate = useAgentGate("ticketingStatusCatalogRead");
  const associationsReadGate = useAgentGate("ticketingAssociationsRead");
  const refreshGate = useAgentGate("ticketingRefresh");
  const containersReadGate = useAgentGate("ticketingContainersRead");
  const columnsSyncGate = useAgentGate("ticketingColumnsSync");
  const statusCatalogRefreshGate = useAgentGate("ticketingStatusCatalogRefresh");
  const statusPresentationUpdateGate = useAgentGate("ticketingStatusPresentationUpdate");
  const ticketsReadGate = useAgentGate("ticketingTicketsRead");
  const filterOptionsReadGate = useAgentGate("ticketingFilterOptionsRead");
  const detailReadGate = useAgentGate("ticketingDetailRead");
  const transitionsReadGate = useAgentGate("ticketingTransitionsRead");
  const transitionWriteGate = useAgentGate("ticketingTransitionWrite");
  const assignWriteGate = useAgentGate("ticketingAssignWrite");
  const clearAssigneeWriteGate = useAgentGate("ticketingClearAssigneeWrite");
  const commentWriteGate = useAgentGate("ticketingCommentWrite");
  const labelsWriteGate = useAgentGate("ticketingLabelsWrite");
  const labelsReadGate = useAgentGate("ticketingLabelsRead");
  const projectsQuery = useProjects();
  const providersQuery = useTicketingProviders(projectId, {
    enabled: Boolean(projectId) && !providersReadGate.gated,
  });
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
    { enabled: Boolean(activeProvider && readableProvider) && !containersReadGate.gated },
  );
  const topLevelContainers = useMemo(() => containersQuery.data ?? [], [containersQuery.data]);
  const selectedSpaceFromTopLevel = useMemo(
    () => activeProvider === "clickup" ? clickupSpaceForContainer(activeContainerId, topLevelContainers) : null,
    [activeContainerId, activeProvider, topLevelContainers],
  );
  const clickupChildContainersQuery = useTicketingContainers(
    activeProvider === "clickup" && selectedSpaceFromTopLevel
      ? { provider: "clickup", projectId, parentContainerId: selectedSpaceFromTopLevel }
      : null,
    { enabled: Boolean(activeProvider === "clickup" && selectedSpaceFromTopLevel && readableProvider) && !containersReadGate.gated },
  );
  const containers = useMemo(
    () => activeProvider === "clickup"
      ? [...topLevelContainers, ...(clickupChildContainersQuery.data ?? [])]
      : topLevelContainers,
    [activeProvider, clickupChildContainersQuery.data, topLevelContainers],
  );
  const clickupSpaceContainers = useMemo(
    () => containers.filter((container) => container.kind === "space"),
    [containers],
  );
  const activeContainer = useMemo(
    () => containers.find((container) => container.id === activeContainerId) ?? null,
    [activeContainerId, containers],
  );
  const activeContainerName = activeContainer?.name ?? null;
  const selectedClickUpSpaceId = useMemo(
    () => activeProvider === "clickup" ? clickupSpaceForContainer(activeContainerId, containers) : null,
    [activeContainerId, activeProvider, containers],
  );
  const clickupLocationChildren = useMemo(
    () => activeProvider === "clickup" ? clickupChildLocations(selectedClickUpSpaceId, containers) : [],
    [activeProvider, containers, selectedClickUpSpaceId],
  );
  const filterBarContainers = activeProvider === "clickup" ? clickupSpaceContainers : containers;
  const filterBarContainerId = activeProvider === "clickup" ? selectedClickUpSpaceId : activeContainerId;

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

  const columnsContainerId = activeContainerId;
  const statusCatalogScope = useMemo(
    () => statusCatalogScopeForSelection(activeProvider, columnsContainerId, activeContainer),
    [activeContainer, activeProvider, columnsContainerId],
  );
  const columnsQuery = useTicketingColumns(
    activeProvider && !containerSelectionNeeded
      ? {
          provider: activeProvider,
          ...(columnsContainerId !== null && { containerId: columnsContainerId }),
        }
      : null,
    { enabled: Boolean(activeProvider && readableProvider && !containerSelectionNeeded) && !columnsSyncGate.gated },
  );
  const columns = columnsQuery.data ?? [];

  const ticketFilters = toTicketFilters(filters);
  const filterOptionFilters = toTicketFilterOptionFilters(filters);
  const ticketContainerId = activeContainerId;
  const ticketQuery: ListTicketsInput | null = activeProvider && readableProvider && !containerSelectionNeeded
    ? {
        provider: activeProvider,
        projectId,
        limit: 40,
        sort: "updated_desc",
        ...(ticketContainerId !== null && { containerId: ticketContainerId }),
        ...(ticketFilters !== undefined && { filters: ticketFilters }),
      }
    : null;

  const ticketsQuery = useTickets(ticketQuery, { enabled: Boolean(ticketQuery) && !ticketsReadGate.gated });
  const tickets = useMemo(() => flattenTicketPages(ticketsQuery.data), [ticketsQuery.data]);
  const filterOptionsInput: ListTicketFilterOptionsInput | null = activeProvider && activeProvider !== "clickup" && readableProvider && !containerSelectionNeeded
    ? {
        provider: activeProvider,
        projectId,
        limit: 500,
        ...(ticketContainerId !== null && { containerId: ticketContainerId }),
        ...(filterOptionFilters !== undefined && { filters: filterOptionFilters }),
      }
    : null;
  const filterOptionsQuery = useTicketFilterOptions(filterOptionsInput, {
    enabled: Boolean(filterOptionsInput) && !filterOptionsReadGate.gated,
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
  const clickupListSprintOptions = useMemo(
    () =>
      activeProvider === "clickup"
        ? clickupLocationChildren
            .filter((location) => location.kind === "list")
            .map((location) => location.name.trim())
            .filter(Boolean)
        : [],
    [activeProvider, clickupLocationChildren],
  );
  const assigneeOptions = useMemo(() => {
    const options = activeProvider === "clickup"
      ? pageAssigneeOptions
      : filterOptionsQuery.data?.assignees ?? pageAssigneeOptions;
    const selectedAssignees = filters.assignees
      .map((assignee) => assignee.trim())
      .filter((assignee) => assignee && assignee !== UNASSIGNED_ASSIGNEE);
    const missingSelected = selectedAssignees.filter((assignee) => !options.includes(assignee));
    if (missingSelected.length === 0) {
      return options;
    }
    return [...options, ...missingSelected].sort((left, right) => left.localeCompare(right));
  }, [activeProvider, filterOptionsQuery.data?.assignees, filters.assignees, pageAssigneeOptions]);
  const sprintOptions = useMemo(
    () => {
      if (activeProvider !== "clickup") {
        return [];
      }
      return Array.from(new Set([...clickupListSprintOptions, ...pageSprintOptions]))
        .sort((left, right) => left.localeCompare(right));
    },
    [activeProvider, clickupListSprintOptions, pageSprintOptions],
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
  const selectedClickUpSpace = useMemo(
    () => clickupSpaceContainers.find((space) => space.id === selectedClickUpSpaceId) ?? null,
    [clickupSpaceContainers, selectedClickUpSpaceId],
  );
  const statusCatalogScopeName = useMemo(
    () =>
      statusCatalogScopeLabel(
        activeProvider,
        statusCatalogScope,
        activeContainerName,
      ),
    [activeContainerName, activeProvider, statusCatalogScope],
  );
  // Jira and ClickUp scope tickets to the selected container server-side (issues
  // carry no matching `project` field), so the client-side container-name filter
  // must be skipped for them; Linear returns all issues and relies on this
  // client filter.
  const clientContainerFilter =
    activeProvider === "jira" || activeProvider === "clickup" ? null : activeContainerName;
  const displayedTickets = useMemo(
    () =>
      filterTicketsByProject(tickets, clientContainerFilter),
    [tickets, clientContainerFilter],
  );
  const statusTicketCounts = useMemo(
    () => ticketStatusCountsByKey(displayedTickets),
    [displayedTickets],
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
    : mergeProviderAndTicketColumns(columns, effectiveTicketColumns);
  const selectedSummary = selectedTicketRef
    ? tickets.find((ticket) => ticketRefsIdentifySameTicket(ticket.ref, selectedTicketRef)) ?? null
    : null;
  const selectedPullRequestSelector =
    pullRequestSelectorFromShell(selectedPullRequestShell);
  const shouldHydrateKanban = useAfterPaint(viewMode === "kanban");
  const shouldHydrateDetail = useAfterPaint(selectedTicketRef !== null);
  const detailInput = selectedTicketRef && activeProvider && shouldHydrateDetail
    ? { provider: activeProvider, ticketRef: selectedTicketRef }
    : null;
  const detailQuery = useTicketDetail(detailInput, { enabled: Boolean(detailInput) && !detailReadGate.gated });
  const transitionsQuery = useTicketTransitions(detailInput, { enabled: Boolean(detailInput) && !transitionsReadGate.gated });
  // Linear pick-list needs the issue team's labels; Jira is free-text and skips it.
  const labelOptionsQuery = useTicketLabelOptions(detailInput, {
    enabled: Boolean(detailInput) && activeProvider === "linear" && !labelsReadGate.gated,
  });
  const associationsQuery = useTicketAssociations(
    detailInput ? { ...detailInput, projectId } : null,
    { enabled: Boolean(detailInput && projectId) && !associationsReadGate.gated },
  );
  const refreshTickets = useRefreshTickets();
  const statusCatalogQuery = useTicketingStatusCatalog(statusCatalogScope, {
    enabled:
      statusManagerOpen &&
      Boolean(statusCatalogScope && readableProvider) &&
      !statusCatalogReadGate.gated,
  });
  const refreshStatusCatalog = useRefreshTicketingStatusCatalog();
  const updateStatusPresentationMutation = useUpdateTicketingStatusPresentation();
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
  const statusManagerError = statusCatalogQuery.isError
    ? queryErrorDetail(statusCatalogQuery.error, "Statuses could not be loaded.")
    : refreshStatusCatalog.isError
      ? queryErrorDetail(refreshStatusCatalog.error, "Statuses could not be synced.")
      : updateStatusPresentationMutation.isError
        ? queryErrorDetail(updateStatusPresentationMutation.error, "Status presentation could not be saved.")
        : null;

  useEffect(() => {
    if (!statusManagerOpen) {
      lastAutoSyncedStatusScope.current = null;
      return;
    }
    if (!statusCatalogScope || !readableProvider || statusCatalogRefreshGate.gated) {
      return;
    }
    const scopeKey = statusCatalogScopeKey(statusCatalogScope);
    if (!scopeKey || lastAutoSyncedStatusScope.current === scopeKey) {
      return;
    }
    lastAutoSyncedStatusScope.current = scopeKey;
    refreshStatusCatalog.mutate(statusCatalogScope);
  }, [readableProvider, refreshStatusCatalog, statusCatalogRefreshGate.gated, statusCatalogScope, statusManagerOpen]);

  function handleBindConversation(conversationId: string) {
    bindConversation.mutate(conversationId);
  }

  function handleSyncStatuses() {
    if (!statusCatalogScope || statusCatalogRefreshGate.gated) {
      return;
    }
    refreshStatusCatalog.mutate(statusCatalogScope);
  }

  function handleMoveStatus(entry: TicketingStatusCatalogEntry, direction: -1 | 1) {
    if (!statusCatalogScope || statusPresentationUpdateGate.gated) {
      return;
    }
    const entries = sortedStatusCatalogEntries(statusCatalogQuery.data ?? []);
    const currentIndex = entries.findIndex((item) => item.providerStatusId === entry.providerStatusId);
    const target = entries[currentIndex + direction];
    if (!target) {
      return;
    }
    updateStatusPresentationMutation.mutate({
      ...statusCatalogScope,
      patches: [
        {
          providerStatusId: entry.providerStatusId,
          displayOrder: target.displayOrder,
        },
        {
          providerStatusId: target.providerStatusId,
          displayOrder: entry.displayOrder,
        },
      ],
    });
  }

  function handleStatusColorChange(entry: TicketingStatusCatalogEntry, color: string) {
    if (!statusCatalogScope || statusPresentationUpdateGate.gated) {
      return;
    }
    updateStatusPresentationMutation.mutate({
      ...statusCatalogScope,
      patches: [{ providerStatusId: entry.providerStatusId, colorOverride: color }],
    });
  }

  function handleResetStatusColor(entry: TicketingStatusCatalogEntry) {
    if (!statusCatalogScope || statusPresentationUpdateGate.gated) {
      return;
    }
    updateStatusPresentationMutation.mutate({
      ...statusCatalogScope,
      patches: [{ providerStatusId: entry.providerStatusId, colorOverride: null }],
    });
  }

  function handleStatusVisibilityChange(
    entry: TicketingStatusCatalogEntry,
    visible: boolean,
  ) {
    if (!statusCatalogScope || statusPresentationUpdateGate.gated) {
      return;
    }
    updateStatusPresentationMutation.mutate({
      ...statusCatalogScope,
      patches: [{ providerStatusId: entry.providerStatusId, isVisible: visible }],
    });
  }

  // Only treat the loaded detail as the selected ticket when it actually matches
  // the current selection, so switching tickets never flashes the previous one.
  const detailMatchesSelection = Boolean(
    detailQuery.data
    && selectedTicketRef
    && ticketRefsIdentifySameTicket(detailQuery.data.ref, selectedTicketRef),
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
    : mergeProviderAndTicketColumns(columns, [...effectiveTicketColumns, ...transitionColumns]);
  const providerName = selectedProvider?.label ?? (activeProvider ? providerLabel(activeProvider) : "Provider");
  const containerLabels = containerLabelsForProvider(activeProvider);
  // ClickUp links are created by ticket-origin starts and Git/PR reconciliation.
  // Keep manual binding hidden until this mutation routes to the provider-neutral
  // external-link command instead of the Linear-specific fallback below.
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

  function handleOpenPullRequestDetail(ticket: TicketSummary) {
    if (ticket.openPrNumber == null) {
      return;
    }
    setSelectedPullRequestShell(
      pullRequestShellFromTicket({
        projectId,
        prNumber: ticket.openPrNumber,
        prUrl: ticket.openPrUrl,
        prStatus: ticket.openPrStatus,
        title: ticket.title,
      }),
    );
  }

  function handleRefresh() {
    if (!activeProvider || refreshGate.gated) {
      return;
    }
    refreshTickets.mutate({
      provider: activeProvider,
      ...(activeContainerId !== null && { containerId: activeContainerId }),
    });
  }

  async function handleTransitionTicket(transition: TicketTransitionOption) {
    if (!selectedTicket || transitionWriteGate.gated) {
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
    if (!selectedTicket || assignWriteGate.gated) {
      return;
    }
    await ticketingMutations.assignToMe({
      provider: selectedTicket.ref.provider,
      ticketRef: selectedTicket.ref,
      projectId,
    });
  }

  async function handleClearAssignee() {
    if (!selectedTicket || clearAssigneeWriteGate.gated) {
      return;
    }
    await ticketingMutations.clearAssignee({
      provider: selectedTicket.ref.provider,
      ticketRef: selectedTicket.ref,
      projectId,
    });
  }

  function handleQuickAssign(ticket: TicketSummary) {
    if (assignWriteGate.gated) {
      return;
    }
    void ticketingMutations
      .assignToMe({ provider: ticket.ref.provider, ticketRef: ticket.ref, projectId })
      .catch((error: unknown) => {
        toast.error(error instanceof Error ? error.message : "Failed to assign ticket");
      });
  }

  async function handleAddComment(bodyMarkdown: string) {
    if (!selectedTicket || commentWriteGate.gated) {
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
    if (!selectedTicket || labelsWriteGate.gated) {
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
    if (transitionWriteGate.gated || transitionsReadGate.gated) {
      return;
    }
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
      .catch((error: unknown) => {
        toast.error(error instanceof Error ? error.message : "Failed to move ticket");
      });
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
        description="Connect a valid Jira, Linear, or ClickUp integration from Settings to browse tickets."
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
        canMoveTickets={Boolean(selectedProvider?.capabilities.kanbanWrite) && !transitionWriteGate.gated && !transitionsReadGate.gated}
        onMoveTicket={handleMoveTicket}
        onSelectTicket={handleSelectTicket}
        isUnread={isTicketUnread}
        canQuickAssign={Boolean(selectedProvider?.capabilities.assignmentWrite) && !assignWriteGate.gated}
        onQuickAssign={handleQuickAssign}
        onOpenPullRequestDetail={handleOpenPullRequestDetail}
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
        canQuickAssign={Boolean(selectedProvider?.capabilities.assignmentWrite) && !assignWriteGate.gated}
        onQuickAssign={handleQuickAssign}
        canMoveTickets={Boolean(selectedProvider?.capabilities.kanbanWrite) && !transitionWriteGate.gated && !transitionsReadGate.gated}
        onMoveTicket={handleMoveTicket}
        onOpenPullRequestDetail={handleOpenPullRequestDetail}
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
        <div className="flex flex-wrap items-center gap-2">
          {validProviders.length > 1 && (
            <ProviderSwitcher
              providers={validProviders}
              activeProvider={activeProvider}
              onProviderChange={setProvider}
            />
          )}
          {activeProvider && (
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={!statusCatalogScope || !readableProvider}
              onClick={() => setStatusManagerOpen(true)}
            >
              <ListOrdered aria-hidden="true" />
              Statuses
            </Button>
          )}
        </div>
      </header>

      <TicketFilterBar
        containers={filterBarContainers}
        columns={filterColumns}
        assigneeOptions={assigneeOptions}
        sprintOptions={sprintOptions}
        showWatcherFilter={
          activeProvider === "clickup" && (hasWatcherMetadata || filters.watcherMe)
        }
        containerLabel={containerLabels.containerLabel}
        allContainersLabel={containerLabels.allContainersLabel}
        activeContainerId={filterBarContainerId}
        containerSelectionNeeded={containerSelectionNeeded}
        filters={filters}
        viewMode={viewMode}
        isRefreshing={refreshTickets.isPending || ticketsQuery.isFetching}
        refreshDisabled={refreshGate.gated}
        onContainerChange={(containerId) => {
          setContainerId(containerId);
          if (activeProvider === "clickup" && containerId !== filterBarContainerId) {
            setFilters({ sprint: null });
          }
        }}
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

      <div className="flex min-h-0 flex-1 overflow-hidden">
        {activeProvider === "clickup" && selectedClickUpSpace && !filters.sprint ? (
          <ClickUpLocationRail
            space={selectedClickUpSpace}
            locations={clickupLocationChildren}
            activeContainerId={activeContainerId}
            onSelect={setContainerId}
          />
        ) : null}
        <div className="min-w-0 flex-1">{content}</div>
      </div>

      <TicketDetailSheet
        open={selectedTicketRef !== null}
        ticket={selectedTicket}
        capabilities={selectedProvider?.capabilities ?? null}
        transitions={transitions}
        associations={associationsQuery.data}
        projectId={projectId}
        isDetailLoading={isDetailPending}
        isAssociationsLoading={associationsQuery.isLoading}
        isTransitionPending={ticketingMutations.transitionStatusMutation.isPending}
        isAssignPending={ticketingMutations.assignToMeMutation.isPending}
        isCommentPending={ticketingMutations.addCommentMutation.isPending}
        isLabelPending={ticketingMutations.setLabelsMutation.isPending}
        labelOptions={labelOptionsQuery.data}
        isLabelOptionsLoading={labelOptionsQuery.isLoading}
        onTransitionTicket={transitionWriteGate.gated ? undefined : handleTransitionTicket}
        onAssignToMe={assignWriteGate.gated ? undefined : handleAssignToMe}
        onClearAssignee={clearAssigneeWriteGate.gated ? undefined : handleClearAssignee}
        onAddComment={commentWriteGate.gated ? undefined : handleAddComment}
        onSetLabels={selectedTicket && !labelsWriteGate.gated ? handleSetLabels : undefined}
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

      <PullRequestDetailSheet
        open={selectedPullRequestShell !== null}
        selector={selectedPullRequestSelector}
        shell={selectedPullRequestShell}
        onNavigateToAssociation={onNavigateToAssociation}
        onClose={() => setSelectedPullRequestShell(null)}
      />

      <StatusManagementDialog
        open={statusManagerOpen}
        providerName={providerName}
        scopeLabel={statusCatalogScopeName}
        entries={statusCatalogQuery.data ?? []}
        ticketCounts={statusTicketCounts}
        isLoading={statusCatalogQuery.isLoading || refreshStatusCatalog.isPending}
        isSyncing={refreshStatusCatalog.isPending || statusCatalogRefreshGate.gated}
        isUpdating={updateStatusPresentationMutation.isPending || statusPresentationUpdateGate.gated}
        error={statusManagerError}
        onSync={handleSyncStatuses}
        onMove={handleMoveStatus}
        onColorChange={handleStatusColorChange}
        onResetColor={handleResetStatusColor}
        onVisibilityChange={handleStatusVisibilityChange}
        onClose={() => setStatusManagerOpen(false)}
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
