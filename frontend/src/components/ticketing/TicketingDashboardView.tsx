import { useEffect, useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Check, Copy, Loader2, Search, ScrollText } from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

import { atlassianApi } from "@/api/atlassian";
import {
  granolaApi,
  type GranolaNoteDetail,
  type GranolaNoteSummary,
} from "@/api/granola";
import { linearApi } from "@/api/linear";
import type { ComposerIntegrationReference } from "@/api/chat";
import type {
  ListTicketFilterOptionsInput,
  ListTicketsInput,
  TicketDeepLink,
  TicketFiltersInput,
  TicketRef,
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
import { pullRequestShellFromTicket, type PullRequestShell } from "@/components/pr/PullRequestDetailBody";
import {
  PullRequestDetailSheet,
} from "@/components/pr/PullRequestDetailSheet";
import {
  pullRequestSelectorFromShell,
} from "@/components/pr/PullRequestDetailPanel";
import { formatRelativeTime } from "@/lib/formatters";
import { markdownComponents } from "@/components/Chat/MessageItem.markdown";
import { invalidateAgentConversationGranolaNote } from "@/components/agents/agentGranolaNoteQueries";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
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

type DashboardSurface = "tickets" | "granola";

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
  return {
    provider: "clickup",
    kind: "clickup",
    id: ticket.ref.id,
    key: ticket.ref.key ?? ticket.ref.id,
    title: ticket.title,
    ...(ticket.url ? { url: ticket.url } : {}),
  };
}

function granolaComposerReference(
  note: GranolaNoteDetail | GranolaNoteSummary,
): ComposerIntegrationReference {
  return {
    provider: "granola",
    kind: "note",
    id: note.id,
    title: note.title ?? note.id,
    ...(note.url ? { url: note.url } : {}),
    ...(note.summary ? { summaryExcerpt: note.summary } : {}),
    includeTranscript: true,
  };
}

function DashboardSurfaceSwitcher({
  activeSurface,
  onSurfaceChange,
}: {
  activeSurface: DashboardSurface;
  onSurfaceChange: (surface: DashboardSurface) => void;
}) {
  const surfaces: Array<{ id: DashboardSurface; label: string }> = [
    { id: "tickets", label: "Tickets" },
    { id: "granola", label: "Granola" },
  ];
  return (
    <div
      className="inline-flex h-8 items-center rounded-md p-0.5"
      role="tablist"
      aria-label="Ticketing content"
      style={{
        backgroundColor: "var(--bg-sunken)",
        borderColor: "var(--border-subtle)",
        borderStyle: "solid",
        borderWidth: "1px",
      }}
    >
      {surfaces.map((surface) => {
        const isActive = surface.id === activeSurface;
        return (
          <button
            key={surface.id}
            type="button"
            role="tab"
            aria-selected={isActive}
            className="h-7 rounded px-3 text-xs font-medium transition-colors focus-visible:outline-none focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]"
            style={{
              backgroundColor: isActive ? "var(--bg-elevated)" : "transparent",
              color: isActive ? "var(--text-primary)" : "var(--text-muted)",
            }}
            onClick={() => onSurfaceChange(surface.id)}
          >
            {surface.label}
          </button>
        );
      })}
    </div>
  );
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

function granolaNoteTimestamp(note: GranolaNoteSummary | GranolaNoteDetail): string | null {
  if ("updatedAt" in note && note.updatedAt) {
    return note.updatedAt;
  }
  if ("createdAt" in note && note.createdAt) {
    return note.createdAt;
  }
  return null;
}

function parseGranolaDate(value: string | null | undefined): Date | null {
  if (!value) {
    return null;
  }
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? null : date;
}

function formatGranolaNoteDate(value: string | null | undefined): string | null {
  const date = parseGranolaDate(value);
  if (!date) {
    return null;
  }
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    ...(date.getFullYear() === new Date().getFullYear() ? {} : { year: "numeric" }),
  }).format(date);
}

function formatGranolaNoteTime(value: string | null | undefined): string | null {
  const date = parseGranolaDate(value);
  if (!date) {
    return null;
  }
  return new Intl.DateTimeFormat(undefined, {
    hour: "numeric",
    minute: "2-digit",
  }).format(date);
}

function GranolaMarkdownBlock({ markdown }: { markdown: string }) {
  return (
    <div className="prose prose-sm max-w-none text-sm leading-6 dark:prose-invert">
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={markdownComponents}>
        {markdown}
      </ReactMarkdown>
    </div>
  );
}

function granolaTranscriptText(note: GranolaNoteDetail | GranolaNoteSummary | null): string {
  if (!note || !("transcript" in note)) {
    return "";
  }
  return note.transcript
    .map((entry) => {
      const text = entry.text.trim();
      if (!text) {
        return "";
      }
      const speaker = entry.speaker?.trim();
      return speaker ? `${speaker}: ${text}` : text;
    })
    .filter(Boolean)
    .join("\n\n");
}

interface GranolaContextDialogProps {
  open: boolean;
  note: GranolaNoteDetail | GranolaNoteSummary | null;
  projects: Project[];
  selectedProjectId: string;
  conversations: { id: string; title: string | null }[];
  selectedConversationId: string;
  isConversationsLoading: boolean;
  isBindPending: boolean;
  bindError: string | null;
  onProjectChange: (projectId: string) => void;
  onConversationChange: (conversationId: string) => void;
  onStartNew: () => void;
  onBindExisting: () => void;
  onClose: () => void;
}

function GranolaContextDialog({
  open,
  note,
  projects,
  selectedProjectId,
  conversations,
  selectedConversationId,
  isConversationsLoading,
  isBindPending,
  bindError,
  onProjectChange,
  onConversationChange,
  onStartNew,
  onBindExisting,
  onClose,
}: GranolaContextDialogProps) {
  return (
    <Dialog open={open} onOpenChange={(nextOpen) => !nextOpen && onClose()}>
      <DialogContent className="sm:max-w-[520px]">
        <DialogHeader className="block space-y-1.5 px-6 py-5 pr-14">
          <DialogTitle className="text-lg leading-6">Add Granola Context</DialogTitle>
          <DialogDescription className="max-w-[34rem] leading-5">
            {note?.title ?? note?.id ?? "Granola note"}
          </DialogDescription>
        </DialogHeader>

        <div className="grid gap-5 px-6 py-5">
          <label className="grid gap-1.5 text-sm">
            <span className="font-medium text-[var(--text-primary)]">Project</span>
            <TicketSearchableSelect
              ariaLabel="Project"
              size="md"
              value={selectedProjectId}
              onValueChange={onProjectChange}
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

          <div className="grid gap-2">
            <div className="flex items-center justify-between gap-3">
              <div className="min-w-0">
                <p className="text-sm font-medium text-[var(--text-primary)]">
                  New conversation
                </p>
                <p className="truncate text-xs text-[var(--text-muted)]">
                  {note?.title ?? note?.id ?? "Granola note"}
                </p>
              </div>
              <Button
                type="button"
                onClick={onStartNew}
                disabled={!note || !selectedProjectId || projects.length === 0}
              >
                Open composer
              </Button>
            </div>
          </div>

          <div className="grid gap-2">
            <label className="grid gap-1.5 text-sm">
              <span className="font-medium text-[var(--text-primary)]">
                Existing conversation
              </span>
              <TicketSearchableSelect
                ariaLabel="Existing conversation"
                size="md"
                value={selectedConversationId}
                onValueChange={onConversationChange}
                placeholder={
                  isConversationsLoading
                    ? "Loading conversations"
                    : conversations.length === 0
                      ? "No conversations"
                      : "Select conversation"
                }
                searchPlaceholder="Search conversations..."
                emptyLabel="No conversations found"
                disabled={isConversationsLoading || conversations.length === 0}
                options={conversations.map((conversation) => ({
                  value: conversation.id,
                  label: conversation.title ?? "Untitled conversation",
                }))}
              />
            </label>
            {bindError ? (
              <p className="text-xs text-[var(--status-error)]">{bindError}</p>
            ) : null}
            <div className="flex justify-end">
              <Button
                type="button"
                variant="outline"
                disabled={!note || !selectedConversationId || isBindPending}
                onClick={onBindExisting}
              >
                {isBindPending ? "Binding..." : "Bind existing conversation"}
              </Button>
            </div>
          </div>
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

function TicketingGranolaNotesView({
  projectId,
  projects,
  onStartConversation,
}: {
  projectId: string;
  projects: Project[];
  onStartConversation: (note: GranolaNoteDetail | GranolaNoteSummary, projectId: string) => void;
}) {
  const [query, setQuery] = useState("");
  const [selectedNoteId, setSelectedNoteId] = useState<string | null>(null);
  const [copiedAction, setCopiedAction] = useState<"summary" | "transcript" | null>(null);
  const [contextDialogOpen, setContextDialogOpen] = useState(false);
  const [contextProjectId, setContextProjectId] = useState(projectId);
  const [selectedConversationId, setSelectedConversationId] = useState("");
  const queryClient = useQueryClient();
  const settingsQuery = useQuery({
    queryKey: ["ticketing", "granola", "settings"] as const,
    queryFn: () => granolaApi.getSettings(),
    staleTime: 30_000,
  });
  const granolaSettings = settingsQuery.data;
  const granolaReady =
    granolaSettings?.enabled === true
    && granolaSettings.validationStatus === "valid";
  const notesQuery = useQuery({
    queryKey: ["ticketing", "granola", "notes"] as const,
    queryFn: () => granolaApi.listNotes({ pageSize: 30 }),
    enabled: granolaReady,
    staleTime: 20_000,
  });
  const notes = useMemo(() => notesQuery.data?.notes ?? [], [notesQuery.data?.notes]);
  const filteredNotes = useMemo(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) {
      return notes;
    }
    return notes.filter((note) =>
      [note.title, note.summary, note.id]
        .filter(Boolean)
        .some((value) => value!.toLowerCase().includes(needle)),
    );
  }, [notes, query]);
  const selectedSummary = selectedNoteId
    ? notes.find((note) => note.id === selectedNoteId) ?? null
    : null;
  const detailQuery = useQuery({
    queryKey: ["ticketing", "granola", "note-detail", selectedNoteId] as const,
    queryFn: () =>
      granolaApi.getNoteDetail({
        noteId: selectedNoteId!,
        includeTranscript: true,
      }),
    enabled: granolaReady && Boolean(selectedNoteId),
    staleTime: 20_000,
  });
  const selectedNote = detailQuery.data ?? selectedSummary;
  const transcriptText = granolaTranscriptText(selectedNote);
  const conversationsQuery = useConversations({
    view: "ticketing",
    projectId: contextProjectId || projectId,
  });
  const bindableConversations = useMemo(
    () =>
      (conversationsQuery.data ?? []).map((conversation) => ({
        id: conversation.id,
        title: conversation.title,
      })),
    [conversationsQuery.data],
  );
  const bindGranolaConversation = useMutation({
    mutationFn: async () => {
      if (!selectedNote || !selectedConversationId) {
        throw new Error("Select a Granola note and conversation.");
      }
      return granolaApi.assignAgentConversationGranolaNote({
        conversationId: selectedConversationId,
        projectId: contextProjectId,
        noteId: selectedNote.id,
        title: selectedNote.title ?? null,
        noteUrl: selectedNote.url ?? null,
        summary: selectedNote.summary ?? null,
        includeTranscript: true,
        refresh: true,
      });
    },
    onSuccess: () => {
      void invalidateAgentConversationGranolaNote(queryClient, selectedConversationId);
      setContextDialogOpen(false);
      setSelectedConversationId("");
    },
  });
  const bindError =
    bindGranolaConversation.error instanceof Error
      ? bindGranolaConversation.error.message
      : bindGranolaConversation.error
        ? "Conversation could not be bound."
        : null;

  useEffect(() => {
    if (!contextDialogOpen) {
      return;
    }
    setContextProjectId(projectId);
    setSelectedConversationId("");
  }, [contextDialogOpen, projectId]);

  async function copyGranolaText(kind: "summary" | "transcript", text: string) {
    const trimmed = text.trim();
    if (!trimmed) {
      return;
    }
    try {
      await navigator.clipboard?.writeText(trimmed);
      setCopiedAction(kind);
      window.setTimeout(() => {
        setCopiedAction((current) => (current === kind ? null : current));
      }, 1600);
    } catch {
      setCopiedAction(null);
    }
  }

  function handleStartContext() {
    if (!selectedNote || !contextProjectId) {
      return;
    }
    onStartConversation(selectedNote, contextProjectId);
    setContextDialogOpen(false);
  }

  if (settingsQuery.isLoading) {
    return (
      <TicketingStatePanel
        state="loading"
        title="Loading Granola"
        description="Checking the Granola connection."
      />
    );
  }

  if (!granolaReady) {
    return (
      <TicketingStatePanel
        state="disconnected"
        title="Granola is not connected"
        description={
          granolaSettings?.lastError
            ?? "Connect and validate Granola from Settings to browse notes."
        }
      />
    );
  }

  return (
    <div
      className="grid min-h-0 flex-1 grid-cols-[minmax(260px,380px)_minmax(0,1fr)] overflow-hidden max-lg:grid-cols-1"
      data-testid="ticketing-granola-notes"
      data-project-id={projectId}
    >
      <aside
        className="flex min-h-0 flex-col border-r max-lg:border-r-0 max-lg:border-b"
        style={{
          backgroundColor: "var(--bg-surface)",
          borderColor: "var(--border-subtle)",
          borderStyle: "solid",
          borderTopWidth: 0,
          borderBottomWidth: 0,
          borderLeftWidth: 0,
          borderRightWidth: 1,
        }}
      >
        <div className="border-b p-3" style={{ borderColor: "var(--border-subtle)" }}>
          <div className="relative">
            <Search
              className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2"
              style={{ color: "var(--text-muted)" }}
            />
            <Input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Search Granola notes"
              className="pl-9"
            />
          </div>
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto p-2">
          {notesQuery.isLoading ? (
            <div className="flex items-center gap-2 px-3 py-2 text-sm text-[var(--text-muted)]">
              <Loader2 className="h-4 w-4 animate-spin" />
              Loading notes
            </div>
          ) : filteredNotes.length === 0 ? (
            <div className="px-3 py-6 text-sm text-[var(--text-muted)]">
              No Granola notes found.
            </div>
          ) : (
            <div className="space-y-1">
              {filteredNotes.map((note) => {
                const selected = note.id === selectedNoteId;
                const timestamp = granolaNoteTimestamp(note);
                const dateLabel = formatGranolaNoteDate(timestamp);
                const timeLabel = formatGranolaNoteTime(timestamp);
                return (
                  <button
                    key={note.id}
                    type="button"
                    className="w-full rounded-md px-3 py-2 text-left transition-colors focus-visible:outline-none focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]"
                    style={{
                      backgroundColor: selected ? "var(--bg-hover)" : "transparent",
                      color: "var(--text-primary)",
                    }}
                    onClick={() => setSelectedNoteId(note.id)}
                  >
                    <span className="flex min-w-0 items-start gap-3">
                      <span className="min-w-0 flex-1">
                        <span className="block truncate text-sm font-medium">
                          {note.title ?? note.id}
                        </span>
                        {dateLabel ? (
                          <span className="mt-1 block text-xs text-[var(--text-muted)]">
                            {dateLabel}
                          </span>
                        ) : null}
                      </span>
                      {timeLabel ? (
                        <span className="shrink-0 pt-0.5 text-xs text-[var(--text-muted)]">
                          {timeLabel}
                        </span>
                      ) : null}
                    </span>
                    {note.summary ? (
                      <span className="mt-1 block line-clamp-2 text-xs text-[var(--text-muted)]">
                        {note.summary}
                      </span>
                    ) : null}
                  </button>
                );
              })}
            </div>
          )}
        </div>
      </aside>

      <section className="min-h-0 overflow-y-auto p-5">
        {!selectedNote ? (
          <div className="flex h-full min-h-[220px] items-center justify-center text-sm text-[var(--text-muted)]">
            Select a Granola note to inspect its details.
          </div>
        ) : (
          <div className="mx-auto flex max-w-3xl flex-col gap-4">
            <div className="flex flex-wrap items-start justify-between gap-3">
              <div className="min-w-0">
                <div className="flex items-center gap-2 text-xs font-medium uppercase text-[var(--text-muted)]">
                  <ScrollText className="h-4 w-4" />
                  Granola note
                </div>
                <h2 className="mt-1 text-xl font-semibold text-[var(--text-primary)]">
                  {selectedNote.title ?? selectedNote.id}
                </h2>
                {granolaNoteTimestamp(selectedNote) ? (
                  <p className="mt-1 text-xs text-[var(--text-muted)]">
                    {[
                      formatGranolaNoteDate(granolaNoteTimestamp(selectedNote)),
                      formatGranolaNoteTime(granolaNoteTimestamp(selectedNote)),
                    ]
                      .filter(Boolean)
                      .join(" at ")}
                  </p>
                ) : null}
              </div>
              <div className="flex flex-wrap items-center gap-2">
                <Button
                  type="button"
                  size="sm"
                  onClick={() => setContextDialogOpen(true)}
                >
                  Add as context
                </Button>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  disabled={!selectedNote.summary?.trim()}
                  onClick={() => void copyGranolaText("summary", selectedNote.summary ?? "")}
                >
                  {copiedAction === "summary" ? (
                    <Check className="mr-2 h-3.5 w-3.5" aria-hidden="true" />
                  ) : (
                    <Copy className="mr-2 h-3.5 w-3.5" aria-hidden="true" />
                  )}
                  {copiedAction === "summary" ? "Copied" : "Copy summary"}
                </Button>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  disabled={!transcriptText}
                  onClick={() => void copyGranolaText("transcript", transcriptText)}
                >
                  {copiedAction === "transcript" ? (
                    <Check className="mr-2 h-3.5 w-3.5" aria-hidden="true" />
                  ) : (
                    <Copy className="mr-2 h-3.5 w-3.5" aria-hidden="true" />
                  )}
                  {copiedAction === "transcript" ? "Copied" : "Copy full transcript"}
                </Button>
              </div>
            </div>

            {detailQuery.isFetching ? (
              <div className="flex items-center gap-2 text-sm text-[var(--text-muted)]">
                <Loader2 className="h-4 w-4 animate-spin" />
                Loading details
              </div>
            ) : null}

            {selectedNote.summary ? (
              <div
                className="rounded-md border p-4"
                style={{
                  backgroundColor: "var(--bg-surface)",
                  borderColor: "var(--border-subtle)",
                  borderStyle: "solid",
                  borderWidth: 1,
                }}
              >
                <GranolaMarkdownBlock markdown={selectedNote.summary} />
              </div>
            ) : null}

            {"transcript" in selectedNote && selectedNote.transcript.length > 0 ? (
              <div className="space-y-2">
                <h3 className="text-sm font-semibold text-[var(--text-primary)]">Transcript</h3>
                <div className="space-y-2">
                  {selectedNote.transcript.map((entry, index) => (
                    <div
                      key={`${entry.startMs ?? index}:${index}`}
                      className="rounded-md border p-3 text-sm"
                      style={{
                        backgroundColor: "var(--bg-surface)",
                        borderColor: "var(--border-subtle)",
                        borderStyle: "solid",
                        borderWidth: 1,
                      }}
                    >
                      {entry.speaker ? (
                        <div className="mb-1 text-xs font-medium text-[var(--text-muted)]">
                          {entry.speaker}
                        </div>
                      ) : null}
                      <p className="leading-6 text-[var(--text-primary)]">{entry.text}</p>
                    </div>
                  ))}
                </div>
              </div>
            ) : null}
          </div>
        )}
      </section>
      <GranolaContextDialog
        open={contextDialogOpen}
        note={selectedNote}
        projects={projects}
        selectedProjectId={contextProjectId}
        conversations={bindableConversations}
        selectedConversationId={selectedConversationId}
        isConversationsLoading={conversationsQuery.isLoading}
        isBindPending={bindGranolaConversation.isPending}
        bindError={bindError}
        onProjectChange={(nextProjectId) => {
          setContextProjectId(nextProjectId);
          setSelectedConversationId("");
        }}
        onConversationChange={setSelectedConversationId}
        onStartNew={handleStartContext}
        onBindExisting={() => bindGranolaConversation.mutate()}
        onClose={() => setContextDialogOpen(false)}
      />
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
  const [selectedPullRequestShell, setSelectedPullRequestShell] =
    useState<PullRequestShell | null>(null);
  const [seenBaseline, setSeenBaseline] = useState<string | null>(null);
  const [activeSurface, setActiveSurface] = useState<DashboardSurface>("tickets");
  const [startWorkSelection, setStartWorkSelection] = useState<StartWorkSelection>({
    projectId,
  });
  const showingTickets = activeSurface === "tickets";

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
    ? tickets.find((ticket) => ticketRefsIdentifySameTicket(ticket.ref, selectedTicketRef)) ?? null
    : null;
  const selectedPullRequestSelector =
    pullRequestSelectorFromShell(selectedPullRequestShell);
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

  function handleStartWorkFromGranolaNote(
    note: GranolaNoteDetail | GranolaNoteSummary,
    targetProjectId: string,
  ) {
    setStartConversationDraft({
      projectId: targetProjectId,
      content: "",
      mode: "edit",
      composerIntegrationReferences: [granolaComposerReference(note)],
    });
    setFocusedAgentProject(targetProjectId);
    clearAgentSelection();
    setActiveConversation(`project:${targetProjectId}`, null);
    setCurrentView("agents");
  }

  let content: React.ReactNode;

  if (!showingTickets) {
    content = (
      <TicketingGranolaNotesView
        projectId={projectId}
        projects={projectsQuery.data ?? []}
        onStartConversation={handleStartWorkFromGranolaNote}
      />
    );
  } else if (providersQuery.isLoading) {
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
        canQuickAssign={Boolean(selectedProvider?.capabilities.assignmentWrite)}
        onQuickAssign={handleQuickAssign}
        canMoveTickets={Boolean(selectedProvider?.capabilities.kanbanWrite)}
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
            {showingTickets ? (
              <Badge
                data-testid="ticketing-visible-count"
                aria-label={`${displayedTickets.length} visible ${displayedTickets.length === 1 ? "ticket" : "tickets"}`}
                variant="outline"
                className="h-5 rounded-full border-[var(--border-subtle)] bg-[var(--bg-elevated)] px-2 text-xs font-medium leading-none text-[var(--text-muted)]"
              >
                {displayedTickets.length}
              </Badge>
            ) : null}
          </h1>
          <p className="mt-0.5 text-xs text-[var(--text-muted)]">
            Browse provider tickets and inspect RalphX associations.
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <DashboardSurfaceSwitcher
            activeSurface={activeSurface}
            onSurfaceChange={setActiveSurface}
          />
          {showingTickets && validProviders.length > 1 && (
            <ProviderSwitcher
              providers={validProviders}
              activeProvider={activeProvider}
              onProviderChange={setProvider}
            />
          )}
        </div>
      </header>

      {showingTickets ? (
        <>
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
        </>
      ) : null}

      <div className="flex min-h-0 flex-1 overflow-hidden">{content}</div>

      <TicketDetailSheet
        open={showingTickets && selectedTicketRef !== null}
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

      <PullRequestDetailSheet
        open={selectedPullRequestShell !== null}
        selector={selectedPullRequestSelector}
        shell={selectedPullRequestShell}
        onClose={() => setSelectedPullRequestShell(null)}
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
