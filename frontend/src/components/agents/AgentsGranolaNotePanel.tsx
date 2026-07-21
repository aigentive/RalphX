import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ExternalLink, Loader2, RefreshCw, Search, ScrollText, Unlink } from "lucide-react";
import { useCallback, useMemo, useState, type ReactNode } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { toast } from "sonner";

import {
  granolaApi,
  type AgentConversationGranolaNote,
  type GranolaNoteDetail,
  type GranolaNoteSummary,
} from "@/api/granola";
import { markdownComponents } from "@/components/Chat/MessageItem.markdown";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";

import { agentGranolaNoteKeys } from "./agentGranolaNoteQueries";
import { ArtifactSelectableRegion } from "./artifact-selection/ArtifactSelectableRegion";

interface AgentsGranolaNotePanelProps {
  conversationId: string | null;
  projectId: string | null;
}

type RefreshGranolaNoteOptions = {
  silent?: boolean;
};

export function AgentsGranolaNotePanel({
  conversationId,
  projectId,
}: AgentsGranolaNotePanelProps) {
  const queryClient = useQueryClient();
  const [query, setQuery] = useState("");
  const [selectedNoteId, setSelectedNoteId] = useState<string | null>(null);
  const [isReassigning, setIsReassigning] = useState(false);
  const noteQuery = useQuery({
    queryKey: agentGranolaNoteKeys.note(conversationId),
    queryFn: () =>
      granolaApi.getAgentConversationGranolaNote({
        conversationId: conversationId!,
      }),
    enabled: Boolean(conversationId),
    staleTime: 5_000,
  });
  const note = noteQuery.data ?? null;
  const showList = !note || isReassigning;
  const notesQuery = useQuery({
    queryKey: ["agents", "granola-notes", showList] as const,
    queryFn: () => granolaApi.listNotes({ pageSize: 30 }),
    enabled: Boolean(conversationId) && showList,
    staleTime: 20_000,
  });
  const notes = useMemo(() => notesQuery.data?.notes ?? [], [notesQuery.data?.notes]);
  const filteredNotes = useMemo(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) return notes;
    return notes.filter((resource) =>
      [resource.title, resource.summary, resource.id]
        .filter(Boolean)
        .some((value) => value!.toLowerCase().includes(needle)),
    );
  }, [notes, query]);
  const selectedSummary = useMemo(
    () =>
      selectedNoteId
        ? notes.find((resource) => resource.id === selectedNoteId) ?? null
        : null,
    [notes, selectedNoteId],
  );
  const detailQuery = useQuery({
    queryKey: ["agents", "granola-note-detail", selectedNoteId] as const,
    queryFn: () =>
      granolaApi.getNoteDetail({
        noteId: selectedNoteId!,
        includeTranscript: true,
      }),
    enabled: Boolean(conversationId && showList && selectedNoteId),
    staleTime: 20_000,
  });
  const selectedDetail = detailQuery.data ?? null;
  const assignMutation = useMutation({
    mutationFn: (resource: GranolaNoteSummary) =>
      granolaApi.assignAgentConversationGranolaNote({
        conversationId: conversationId!,
        projectId,
        noteId: resource.id,
        title: selectedDetail?.title ?? resource.title ?? null,
        noteUrl: selectedDetail?.url ?? resource.url ?? null,
        summary: selectedDetail?.summary ?? resource.summary ?? null,
        includeTranscript: true,
      }),
    onSuccess: (assigned) => {
      queryClient.setQueryData(agentGranolaNoteKeys.note(conversationId), assigned);
      setIsReassigning(false);
      setSelectedNoteId(null);
      setQuery("");
      toast.success("Granola note bound");
    },
    onError: (err) => {
      toast.error(err instanceof Error ? err.message : "Failed to bind Granola note");
    },
  });
  const refreshMutation = useMutation({
    mutationFn: (_options?: RefreshGranolaNoteOptions) =>
      granolaApi.refreshAgentConversationGranolaNote({
        conversationId: conversationId!,
      }),
    onSuccess: (refreshed, options) => {
      queryClient.setQueryData(agentGranolaNoteKeys.note(conversationId), refreshed);
      if (!options?.silent) {
        toast.success("Granola note refreshed");
      }
    },
    onError: (err) => {
      toast.error(err instanceof Error ? err.message : "Failed to refresh Granola note");
    },
  });
  const clearMutation = useMutation({
    mutationFn: () =>
      granolaApi.clearAgentConversationGranolaNote({
        conversationId: conversationId!,
      }),
    onSuccess: () => {
      queryClient.setQueryData(agentGranolaNoteKeys.note(conversationId), null);
      setIsReassigning(false);
      setSelectedNoteId(null);
      toast.success("Granola note unlinked");
    },
    onError: (err) => {
      toast.error(err instanceof Error ? err.message : "Failed to unlink Granola note");
    },
  });
  const isMutating =
    assignMutation.isPending || refreshMutation.isPending || clearMutation.isPending;
  const handleAssign = useCallback(
    (resource: GranolaNoteSummary) => {
      if (!conversationId || isMutating) return;
      assignMutation.mutate(resource);
    },
    [assignMutation, conversationId, isMutating],
  );

  return (
    <div
      className="flex h-full min-h-0 flex-col"
      style={{
        backgroundColor: "var(--bg-base)",
        color: "var(--text-primary)",
      }}
    >
      <div
        className="flex items-center justify-between gap-3 border-b px-4 py-3"
        style={{
          borderColor: "var(--border-subtle)",
          borderStyle: "solid",
          borderWidth: 0,
          borderBottomWidth: 1,
        }}
      >
        <div className="flex min-w-0 items-center gap-2">
          <ScrollText className="h-4 w-4 shrink-0" style={{ color: "var(--accent-primary)" }} />
          <div className="min-w-0">
            <h2 className="truncate text-sm font-semibold">Granola</h2>
            <p className="truncate text-xs" style={{ color: "var(--text-muted)" }}>
              {note ? note.title ?? note.noteId : "No note bound"}
            </p>
          </div>
        </div>
        {note ? (
          <div className="flex items-center gap-1">
            {note.noteUrl ? (
              <IconButton
                label="Open Granola note"
                onClick={() => window.open(note.noteUrl ?? undefined, "_blank", "noopener,noreferrer")}
              >
                <ExternalLink className="h-4 w-4" />
              </IconButton>
            ) : null}
            <IconButton
              label="Refresh Granola note"
              onClick={() => refreshMutation.mutate({ silent: false })}
              disabled={!conversationId || isMutating}
            >
              <RefreshCw className={cn("h-4 w-4", refreshMutation.isPending && "animate-spin")} />
            </IconButton>
            <IconButton
              label="Unlink Granola note"
              onClick={() => clearMutation.mutate()}
              disabled={!conversationId || isMutating}
            >
              <Unlink className="h-4 w-4" />
            </IconButton>
          </div>
        ) : null}
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto p-4">
        {!conversationId ? (
          <PanelStatus label="No conversation selected" busy={false} />
        ) : noteQuery.isLoading ? (
          <PanelStatus label="Loading Granola note" />
        ) : note ? (
          <BoundNoteDetails note={note} onReassign={() => setIsReassigning(true)} />
        ) : null}

        {conversationId && showList ? (
          <div className={cn("space-y-3", note && "mt-4 border-t pt-4")} style={{ borderColor: "var(--border-subtle)" }}>
            <div className="relative">
              <Search className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2" style={{ color: "var(--text-muted)" }} />
              <Input
                value={query}
                onChange={(event) => setQuery(event.target.value)}
                placeholder="Filter Granola notes"
                className="pl-9"
              />
            </div>
            {notesQuery.isFetching ? <PanelStatus label="Loading Granola notes" /> : null}
            <div className="grid gap-3 lg:grid-cols-[minmax(0,0.9fr)_minmax(0,1.1fr)]">
              <div className="space-y-2">
                {filteredNotes.map((resource) => (
                  <button
                    key={resource.id}
                    type="button"
                    className="w-full rounded-md border px-3 py-2 text-left transition-colors"
                    style={{
                      backgroundColor:
                        selectedNoteId === resource.id
                          ? "var(--bg-hover)"
                          : "var(--bg-surface)",
                      borderColor: "var(--border-subtle)",
                      borderStyle: "solid",
                      borderWidth: 1,
                    }}
                    onClick={() => setSelectedNoteId(resource.id)}
                  >
                    <span className="block truncate text-sm font-medium">
                      {resource.title ?? resource.id}
                    </span>
                    {resource.summary ? (
                      <span className="mt-1 block line-clamp-2 text-xs" style={{ color: "var(--text-muted)" }}>
                        {resource.summary}
                      </span>
                    ) : null}
                  </button>
                ))}
                {!notesQuery.isFetching && filteredNotes.length === 0 ? (
                  <PanelStatus label="No Granola notes found" busy={false} />
                ) : null}
              </div>
              <NotePreview
                detail={selectedDetail}
                summary={selectedSummary}
                isLoading={detailQuery.isFetching}
                isAssigning={assignMutation.isPending}
                onAssign={handleAssign}
              />
            </div>
          </div>
        ) : null}
      </div>
    </div>
  );
}

function BoundNoteDetails({
  note,
  onReassign,
}: {
  note: AgentConversationGranolaNote;
  onReassign: () => void;
}) {
  return (
    <ArtifactSelectableRegion
      className="space-y-3"
      source={{
        sourceKind: "granola",
        sourceId: note.noteId,
        sourceLabel: "Granola note",
        ...(note.title ? { title: note.title } : {}),
        ...(note.noteUrl ? { url: note.noteUrl } : {}),
      }}
    >
      <div className="min-w-0">
        <h3 className="text-sm font-semibold">{note.title ?? note.noteId}</h3>
        <p className="mt-1 text-xs" style={{ color: "var(--text-muted)" }}>
          {note.refreshStatus === "loaded"
            ? "Bound to this conversation"
            : note.refreshStatus === "error"
              ? "Bound, refresh failed"
              : "Bound, not loaded yet"}
        </p>
      </div>
      {note.summaryMarkdown ? <MarkdownBlock markdown={note.summaryMarkdown} /> : null}
      {note.refreshError ? (
        <p className="text-xs" style={{ color: "var(--danger)" }}>
          {note.refreshError}
        </p>
      ) : null}
      <Button type="button" variant="outline" size="sm" onClick={onReassign}>
        Choose another note
      </Button>
    </ArtifactSelectableRegion>
  );
}

function NotePreview({
  detail,
  summary,
  isLoading,
  isAssigning,
  onAssign,
}: {
  detail: GranolaNoteDetail | null;
  summary: GranolaNoteSummary | null;
  isLoading: boolean;
  isAssigning: boolean;
  onAssign: (resource: GranolaNoteSummary) => void;
}) {
  if (!summary) {
    return <PanelStatus label="Select a note to preview" busy={false} />;
  }
  return (
    <div
      className="min-h-[240px] rounded-md border p-3"
      style={{
        backgroundColor: "var(--bg-surface)",
        borderColor: "var(--border-subtle)",
        borderStyle: "solid",
        borderWidth: 1,
      }}
    >
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <h3 className="text-sm font-semibold">{detail?.title ?? summary.title ?? summary.id}</h3>
          <p className="mt-1 truncate text-xs" style={{ color: "var(--text-muted)" }}>
            {summary.id}
          </p>
        </div>
        <Button
          type="button"
          size="sm"
          onClick={() => onAssign(summary)}
          disabled={isAssigning}
        >
          {isAssigning ? <Loader2 className="mr-2 h-3.5 w-3.5 animate-spin" /> : null}
          Bind
        </Button>
      </div>
      {isLoading ? <PanelStatus label="Loading note detail" /> : null}
      {detail?.summary ?? summary.summary ? (
        <div className="mt-3">
          <MarkdownBlock markdown={detail?.summary ?? summary.summary ?? ""} />
        </div>
      ) : null}
      {detail?.transcript.length ? (
        <div className="mt-3 space-y-2">
          {detail.transcript.slice(0, 8).map((entry, index) => (
            <div key={`${entry.startMs ?? index}:${index}`} className="text-xs">
              {entry.speaker ? (
                <span className="font-medium">{entry.speaker}: </span>
              ) : null}
              <span style={{ color: "var(--text-secondary)" }}>{entry.text}</span>
            </div>
          ))}
        </div>
      ) : null}
    </div>
  );
}

function MarkdownBlock({ markdown }: { markdown: string }) {
  return (
    <div
      className="theme-aware-prose prose prose-sm max-w-none text-sm"
      data-testid="agent-granola-note-markdown"
    >
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={markdownComponents}>
        {markdown}
      </ReactMarkdown>
    </div>
  );
}

function PanelStatus({ label, busy = true }: { label: string; busy?: boolean }) {
  return (
    <div className="flex min-h-[120px] items-center justify-center gap-2 text-sm" style={{ color: "var(--text-muted)" }}>
      {busy ? <Loader2 className="h-4 w-4 animate-spin" /> : null}
      <span>{label}</span>
    </div>
  );
}

function IconButton({
  label,
  onClick,
  disabled,
  children,
}: {
  label: string;
  onClick: () => void;
  disabled?: boolean;
  children: ReactNode;
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          type="button"
          aria-label={label}
          disabled={disabled}
          className="inline-flex h-8 w-8 items-center justify-center rounded-md transition-colors hover:bg-[var(--bg-hover)] disabled:cursor-not-allowed disabled:opacity-50"
          onClick={onClick}
        >
          {children}
        </button>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}
