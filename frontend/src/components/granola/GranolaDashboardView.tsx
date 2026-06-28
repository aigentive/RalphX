import { useEffect, useMemo, useState, type CSSProperties } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  CalendarClock,
  Check,
  Copy,
  Loader2,
  RefreshCw,
  ScrollText,
  Search,
} from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

import {
  granolaApi,
  type GranolaNoteDetail,
  type GranolaNoteSummary,
} from "@/api/granola";
import { invalidateAgentConversationGranolaNote } from "@/components/agents/agentGranolaNoteQueries";
import { markdownComponents } from "@/components/Chat/MessageItem.markdown";
import { TicketSearchableSelect } from "@/components/ticketing/TicketSearchableSelect";
import { TicketingStatePanel } from "@/components/ticketing/TicketingStatePanel";
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
import { useConversations } from "@/hooks/useChat";
import { cn } from "@/lib/utils";
import type { Project } from "@/types/project";

import { GranolaIcon } from "./GranolaIcon";
import { granolaDashboardKeys } from "./granolaDashboardKeys";

type GranolaNoteFilter = "all" | "with_summary" | "without_summary";
type GranolaDateGroup = "today" | "yesterday" | "this_week" | "older" | "undated";

const EMPTY_NOTES: GranolaNoteSummary[] = [];
const GRANOLA_DATE_GROUP_ORDER: GranolaDateGroup[] = [
  "today",
  "yesterday",
  "this_week",
  "older",
  "undated",
];
const GRANOLA_NOTE_FILTERS: Array<{ id: GranolaNoteFilter; label: string }> = [
  { id: "all", label: "All" },
  { id: "with_summary", label: "Summary" },
  { id: "without_summary", label: "No summary" },
];

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

function startOfDay(date: Date): Date {
  const next = new Date(date);
  next.setHours(0, 0, 0, 0);
  return next;
}

function granolaDateGroup(timestamp: string | null): GranolaDateGroup {
  const date = parseGranolaDate(timestamp);
  if (!date) {
    return "undated";
  }
  const today = startOfDay(new Date());
  const noteDay = startOfDay(date);
  const daysAgo = Math.floor((today.getTime() - noteDay.getTime()) / 86_400_000);
  if (daysAgo <= 0) {
    return "today";
  }
  if (daysAgo === 1) {
    return "yesterday";
  }
  if (daysAgo <= 7) {
    return "this_week";
  }
  return "older";
}

function granolaGroupLabel(group: GranolaDateGroup): string {
  switch (group) {
    case "today":
      return "Today";
    case "yesterday":
      return "Yesterday";
    case "this_week":
      return "This week";
    case "older":
      return "Older";
    case "undated":
      return "Undated";
  }
}

function groupedGranolaNotes(notes: GranolaNoteSummary[]): Record<GranolaDateGroup, GranolaNoteSummary[]> {
  return notes.reduce<Record<GranolaDateGroup, GranolaNoteSummary[]>>(
    (groups, note) => {
      groups[granolaDateGroup(granolaNoteTimestamp(note))].push(note);
      return groups;
    },
    {
      today: [],
      yesterday: [],
      this_week: [],
      older: [],
      undated: [],
    },
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

function granolaNoteMatchesSearch(note: GranolaNoteSummary, query: string): boolean {
  const trimmed = query.trim().toLowerCase();
  if (!trimmed) {
    return true;
  }
  const haystack = [note.title, note.summary, note.id, note.url]
    .filter(Boolean)
    .join(" ")
    .toLowerCase();
  return haystack.includes(trimmed);
}

function granolaNoteMatchesFilter(
  note: GranolaNoteSummary,
  filter: GranolaNoteFilter,
): boolean {
  switch (filter) {
    case "with_summary":
      return Boolean(note.summary?.trim());
    case "without_summary":
      return !note.summary?.trim();
    case "all":
      return true;
  }
}

function noteFilterCount(notes: GranolaNoteSummary[], filter: GranolaNoteFilter): number {
  return notes.filter((note) => granolaNoteMatchesFilter(note, filter)).length;
}

function GranolaFilterButton({
  active,
  children,
  onClick,
}: {
  active: boolean;
  children: React.ReactNode;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      className="inline-flex h-8 items-center gap-1 rounded-md px-3 text-xs font-medium transition-colors focus-visible:outline-none focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]"
      style={{
        backgroundColor: active ? "var(--bg-elevated)" : "transparent",
        borderColor: active ? "var(--border-default)" : "var(--border-subtle)",
        borderStyle: "solid",
        borderWidth: "1px",
        color: active ? "var(--text-primary)" : "var(--text-muted)",
      }}
      onClick={onClick}
    >
      {children}
    </button>
  );
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

function GranolaGroupHeader({
  group,
  count,
}: {
  group: GranolaDateGroup;
  count: number;
}) {
  return (
    <div
      className="sticky top-0 z-10 grid h-9 grid-cols-[18px_minmax(0,1fr)_auto] items-center gap-2 border-b px-4 text-xs font-semibold uppercase tracking-[0.08em]"
      style={{
        backgroundColor: "var(--bg-elevated)",
        borderBottomColor: "var(--border-subtle)",
        borderBottomStyle: "solid",
        borderBottomWidth: "1px",
        color: "var(--text-muted)",
      }}
    >
      <CalendarClock className="h-4 w-4" aria-hidden="true" />
      <span>{granolaGroupLabel(group)}</span>
      <span>{count}</span>
    </div>
  );
}

function GranolaNoteRow({
  note,
  selected,
  onSelect,
}: {
  note: GranolaNoteSummary;
  selected: boolean;
  onSelect: () => void;
}) {
  const timestamp = granolaNoteTimestamp(note);
  const dateLabel = formatGranolaNoteDate(timestamp);
  const timeLabel = formatGranolaNoteTime(timestamp);

  return (
    <button
      type="button"
      data-testid={`granola-note-row-${note.id}`}
      className="grid min-h-[64px] w-full grid-cols-[minmax(0,1fr)_auto] gap-3 border-b px-4 py-3 text-left transition-colors hover:bg-[var(--bg-sunken)] focus-visible:outline-none focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:-2px]"
      style={{
        backgroundColor: selected ? "var(--bg-hover)" : "var(--bg-surface)",
        borderBottomColor: "var(--border-subtle)",
        borderBottomStyle: "solid",
        borderBottomWidth: "1px",
        color: "var(--text-primary)",
      }}
      onClick={onSelect}
    >
      <span className="min-w-0">
        <span className="flex min-w-0 items-center gap-2">
          <ScrollText className="h-4 w-4 shrink-0 text-[var(--text-muted)]" aria-hidden="true" />
          <span className="truncate text-sm font-medium">{note.title ?? note.id}</span>
        </span>
        {note.summary ? (
          <span className="mt-1 block line-clamp-2 text-xs leading-5 text-[var(--text-muted)]">
            {note.summary}
          </span>
        ) : (
          <span className="mt-1 block text-xs text-[var(--text-muted)]">No summary</span>
        )}
      </span>
      <span className="shrink-0 text-right text-xs text-[var(--text-muted)]">
        {dateLabel ? <span className="block">{dateLabel}</span> : null}
        {timeLabel ? <span className="block">{timeLabel}</span> : null}
      </span>
    </button>
  );
}

function EmptyGranolaFilteredState({ onReset }: { onReset: () => void }) {
  return (
    <div className="grid h-full min-h-[320px] place-items-center px-6 text-center">
      <div>
        <p className="text-sm font-semibold text-[var(--text-primary)]">
          No Granola notes match these filters.
        </p>
        <p className="mt-1 text-sm text-[var(--text-muted)]">
          Clear the filters to return to the full note list.
        </p>
        <Button type="button" variant="outline" size="sm" className="mt-4" onClick={onReset}>
          Reset filters
        </Button>
      </div>
    </div>
  );
}

export function GranolaDashboardView({
  projectId,
  project,
  projects,
  onStartConversation,
}: {
  projectId: string;
  project: Project | null;
  projects: Project[];
  onStartConversation: (note: GranolaNoteDetail | GranolaNoteSummary, projectId: string) => void;
}) {
  const [query, setQuery] = useState("");
  const [noteFilter, setNoteFilter] = useState<GranolaNoteFilter>("all");
  const [selectedNoteId, setSelectedNoteId] = useState<string | null>(null);
  const [copiedAction, setCopiedAction] = useState<"summary" | "transcript" | null>(null);
  const [contextDialogOpen, setContextDialogOpen] = useState(false);
  const [contextProjectId, setContextProjectId] = useState(projectId);
  const [selectedConversationId, setSelectedConversationId] = useState("");
  const queryClient = useQueryClient();

  const settingsQuery = useQuery({
    queryKey: granolaDashboardKeys.settings(),
    queryFn: () => granolaApi.getSettings(),
    staleTime: 30_000,
  });
  const granolaSettings = settingsQuery.data;
  const granolaReady =
    granolaSettings?.enabled === true
    && granolaSettings.validationStatus === "valid";
  const notesQuery = useQuery({
    queryKey: granolaDashboardKeys.notes(),
    queryFn: () => granolaApi.listNotes({ pageSize: 30 }),
    enabled: granolaReady,
    staleTime: 20_000,
  });
  const notes = notesQuery.data?.notes ?? EMPTY_NOTES;
  const filteredNotes = useMemo(() => {
    return notes.filter((note) => (
      granolaNoteMatchesSearch(note, query)
      && granolaNoteMatchesFilter(note, noteFilter)
    ));
  }, [noteFilter, notes, query]);
  const groups = useMemo(() => groupedGranolaNotes(filteredNotes), [filteredNotes]);
  const notesWithSummaryCount = noteFilterCount(notes, "with_summary");
  const selectedSummary = selectedNoteId
    ? notes.find((note) => note.id === selectedNoteId) ?? null
    : null;
  const detailQuery = useQuery({
    queryKey: granolaDashboardKeys.noteDetail(selectedNoteId),
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
    view: "granola",
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
    if (selectedNoteId && notes.some((note) => note.id === selectedNoteId)) {
      return;
    }
    setSelectedNoteId(filteredNotes[0]?.id ?? null);
  }, [filteredNotes, notes, selectedNoteId]);

  useEffect(() => {
    if (!contextDialogOpen) {
      return;
    }
    setContextProjectId(projectId);
    setSelectedConversationId("");
  }, [contextDialogOpen, projectId]);

  function resetFilters() {
    setQuery("");
    setNoteFilter("all");
  }

  function handleRefresh() {
    void settingsQuery.refetch();
    void notesQuery.refetch();
    if (selectedNoteId) {
      void detailQuery.refetch();
    }
  }

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

  if (!project) {
    return (
      <TicketingStatePanel
        state="empty"
        title="Project unavailable"
        description="Select a project to browse Granola notes."
      />
    );
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
      data-testid="granola-dashboard-view"
      data-project-id={projectId}
      className="flex h-full min-h-0 flex-col"
      style={{ backgroundColor: "var(--app-content-bg)", color: "var(--text-primary)" }}
    >
      <header
        className="shrink-0 border-b px-5 py-4"
        style={{
          backgroundColor: "var(--bg-surface)",
          borderBottomColor: "var(--border-subtle)",
          borderBottomStyle: "solid",
          borderBottomWidth: "1px",
        }}
      >
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div className="min-w-0">
            <div className="flex min-w-0 items-center gap-2">
              <GranolaIcon className="h-5 w-5 shrink-0" />
              <h1 className="truncate text-lg font-semibold">Granola notes</h1>
            </div>
            <p className="mt-1 truncate text-sm text-[var(--text-muted)]">
              {project.name} - meeting notes and transcript context.
            </p>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <Badge
              variant="outline"
              className="h-6 rounded-full px-2 text-xs font-medium"
              style={{
                backgroundColor: "var(--bg-elevated)",
                borderColor: "var(--border-subtle)",
                borderStyle: "solid",
                borderWidth: "1px",
                color: "var(--text-muted)",
              }}
            >
              {notes.length} notes
            </Badge>
            <Badge
              variant="outline"
              className="h-6 rounded-full px-2 text-xs font-medium"
              style={{
                backgroundColor: "var(--bg-elevated)",
                borderColor: "var(--border-subtle)",
                borderStyle: "solid",
                borderWidth: "1px",
                color: "var(--text-muted)",
              }}
            >
              {notesWithSummaryCount} summaries
            </Badge>
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={handleRefresh}
              disabled={settingsQuery.isFetching || notesQuery.isFetching}
            >
              <RefreshCw
                className={cn(
                  "h-4 w-4",
                  (settingsQuery.isFetching || notesQuery.isFetching) && "animate-spin",
                )}
                aria-hidden="true"
              />
              Refresh
            </Button>
          </div>
        </div>

        <div className="mt-4 flex flex-wrap items-center gap-3">
          <div className="relative min-w-[240px] flex-1">
            <Search
              className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-[var(--text-muted)]"
              aria-hidden="true"
            />
            <Input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Search notes, summaries, or links"
              className="h-8 pl-9 text-sm"
              style={{
                backgroundColor: "var(--bg-elevated)",
                borderColor: "var(--border-subtle)",
                color: "var(--text-primary)",
              }}
            />
          </div>
          <div className="flex flex-wrap items-center gap-1">
            {GRANOLA_NOTE_FILTERS.map((filter) => (
              <GranolaFilterButton
                key={filter.id}
                active={noteFilter === filter.id}
                onClick={() => setNoteFilter(filter.id)}
              >
                {filter.label} {noteFilterCount(notes, filter.id)}
              </GranolaFilterButton>
            ))}
          </div>
        </div>
      </header>

      <div className="grid min-h-0 flex-1 grid-cols-[minmax(320px,44%)_minmax(0,1fr)] max-lg:grid-cols-1">
        <section
          className="grid min-h-0 grid-rows-[auto_1fr] border-r max-lg:border-r-0 max-lg:border-b"
          aria-label="Granola notes"
          style={{
            backgroundColor: "var(--bg-surface)",
            borderRightColor: "var(--border-subtle)",
            borderRightStyle: "solid",
            borderRightWidth: "1px",
          } as CSSProperties}
        >
          <div
            className="grid h-9 grid-cols-[minmax(0,1fr)_88px] items-center gap-3 border-b px-4 text-xs font-semibold uppercase tracking-[0.08em] text-[var(--text-muted)]"
            style={{
              backgroundColor: "var(--bg-surface)",
              borderBottomColor: "var(--border-subtle)",
              borderBottomStyle: "solid",
              borderBottomWidth: "1px",
            }}
          >
            <span>Note</span>
            <span className="text-right">Updated</span>
          </div>
          <div className="min-h-0 overflow-y-auto" data-testid="granola-notes-list">
            {notesQuery.isLoading ? (
              <div className="flex items-center gap-2 px-4 py-3 text-sm text-[var(--text-muted)]">
                <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
                Loading notes
              </div>
            ) : filteredNotes.length === 0 ? (
              <EmptyGranolaFilteredState onReset={resetFilters} />
            ) : (
              GRANOLA_DATE_GROUP_ORDER.map((group) => {
                const groupNotes = groups[group];
                if (groupNotes.length === 0) {
                  return null;
                }
                return (
                  <section key={group} aria-label={`${granolaGroupLabel(group)} Granola notes`}>
                    <GranolaGroupHeader group={group} count={groupNotes.length} />
                    {groupNotes.map((note) => (
                      <GranolaNoteRow
                        key={note.id}
                        note={note}
                        selected={note.id === selectedNoteId}
                        onSelect={() => setSelectedNoteId(note.id)}
                      />
                    ))}
                  </section>
                );
              })
            )}
          </div>
        </section>

        <section className="min-h-0 overflow-y-auto p-5" aria-label="Granola note detail">
          {!selectedNote ? (
            <div className="flex h-full min-h-[220px] items-center justify-center text-sm text-[var(--text-muted)]">
              Select a Granola note to inspect its details.
            </div>
          ) : (
            <div className="mx-auto flex max-w-3xl flex-col gap-4">
              <div className="flex flex-wrap items-start justify-between gap-3">
                <div className="min-w-0">
                  <div className="flex items-center gap-2 text-xs font-medium uppercase text-[var(--text-muted)]">
                    <ScrollText className="h-4 w-4" aria-hidden="true" />
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
                  <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
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
                    borderWidth: "1px",
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
                          borderWidth: "1px",
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
      </div>

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
