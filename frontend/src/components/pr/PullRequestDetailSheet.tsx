import {
  Briefcase,
  ChevronRight,
  GitPullRequestArrow,
  ScrollText,
  Ticket,
  X,
} from "lucide-react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useState, type ReactNode } from "react";

import {
  granolaApi,
  type GranolaNoteSummary,
} from "@/api/granola";
import type {
  PullRequestDetail,
  PullRequestRxConversation,
} from "@/api/github";
import type {
  TicketDeepLink,
  TicketingProvider,
  TicketRef,
  TicketSummary,
} from "@/api/ticketing";
import {
  type PullRequestDetailSelector,
  usePullRequestDetail,
} from "@/hooks/usePullRequestDetail";
import { granolaDashboardKeys } from "@/components/granola/granolaDashboardKeys";
import { invalidateAgentConversationGranolaNote } from "@/components/agents/agentGranolaNoteQueries";
import { TicketDetailSheet } from "@/components/ticketing/TicketDetailSheet";
import { TicketSearchableSelect } from "@/components/ticketing/TicketSearchableSelect";
import { useAfterPaint } from "@/components/ticketing/useAfterPaint";
import { Button } from "@/components/ui/button";
import {
  useTicketAssociations,
  useTicketDetail,
  useTicketTransitions,
} from "@/hooks/useTicketing";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

import {
  PullRequestDetailBody,
} from "./PullRequestDetailBody";
import type { PullRequestShell } from "./PullRequestDetailShell";

type PullRequestTicketAssociation = {
  provider: string;
  issueKey: string;
  title?: string | null | undefined;
  url?: string | null | undefined;
};

type PullRequestTicketSelection = {
  ticketRef: TicketRef;
  fallbackTicket: TicketSummary;
};

function prTicketProviderLabel(provider: string): string {
  switch (provider.toLowerCase()) {
    case "atlassian":
    case "jira":
      return "Jira";
    case "linear":
      return "Linear";
    case "clickup":
      return "ClickUp";
    default:
      return provider;
  }
}

function prTicketProvider(provider: string): TicketingProvider | null {
  switch (provider.toLowerCase()) {
    case "atlassian":
    case "jira":
      return "jira";
    case "linear":
      return "linear";
    case "clickup":
      return "clickup";
    default:
      return null;
  }
}

function normalizeAssociationKey(value: string | null | undefined): string {
  return (value ?? "").trim().toLowerCase();
}

function prGranolaNoteSubtitle(note: GranolaNoteSummary): string | null {
  const parts = [
    (note.rxConversationCount ?? note.rxConversations?.length ?? 0) > 0
      ? `${note.rxConversationCount ?? note.rxConversations?.length ?? 0} RX`
      : null,
    (note.ticketCount ?? note.ticketLinks?.length ?? 0) > 0
      ? `${note.ticketCount ?? note.ticketLinks?.length ?? 0} ticket${
          (note.ticketCount ?? note.ticketLinks?.length ?? 0) === 1 ? "" : "s"
        }`
      : null,
    (note.prCount ?? note.pullRequests?.length ?? 0) > 0
      ? `${note.prCount ?? note.pullRequests?.length ?? 0} PR${
          (note.prCount ?? note.pullRequests?.length ?? 0) === 1 ? "" : "s"
        }`
      : null,
  ].filter(Boolean);
  return parts.length > 0 ? parts.join(" · ") : note.url ?? null;
}

function granolaNoteMatchesPullRequest({
  note,
  prNumber,
  rxConversations,
  linkedTickets,
}: {
  note: GranolaNoteSummary;
  prNumber: number | null;
  rxConversations: PullRequestRxConversation[];
  linkedTickets: PullRequestTicketAssociation[];
}): boolean {
  if (
    prNumber !== null &&
    (note.pullRequests ?? []).some((pullRequest) => pullRequest.number === prNumber)
  ) {
    return true;
  }

  const conversationIds = new Set(
    rxConversations.map((conversation) => normalizeAssociationKey(conversation.conversationId)),
  );
  if (
    conversationIds.size > 0 &&
    (note.rxConversations ?? []).some((conversation) =>
      conversationIds.has(normalizeAssociationKey(conversation.conversationId)),
    )
  ) {
    return true;
  }

  const ticketKeys = new Set(
    linkedTickets.flatMap((ticket) => {
      const provider = normalizeAssociationKey(ticket.provider);
      const issueKey = normalizeAssociationKey(ticket.issueKey);
      return issueKey ? [`${provider}:${issueKey}`, issueKey] : [];
    }),
  );
  return (
    ticketKeys.size > 0 &&
    (note.ticketLinks ?? []).some((ticket) => {
      const provider = normalizeAssociationKey(ticket.provider);
      const label = normalizeAssociationKey(ticket.label);
      return ticketKeys.has(`${provider}:${label}`) || ticketKeys.has(label);
    })
  );
}

function prTicketFallbackSummary(
  ticket: PullRequestTicketAssociation,
): TicketSummary | null {
  const provider = prTicketProvider(ticket.provider);
  if (!provider) {
    return null;
  }
  const ticketRef: TicketRef = {
    provider,
    id: ticket.issueKey,
    key: ticket.issueKey,
  };
  return {
    ref: ticketRef,
    title: ticket.title ?? ticket.issueKey,
    state: {
      id: "linked",
      name: "Linked",
      category: "other",
    },
    assignee: null,
    assignees: [],
    watchers: [],
    reporter: null,
    labels: [],
    sprints: [],
    project: null,
    priority: null,
    updatedAt: new Date(0).toISOString(),
    url: ticket.url ?? null,
    associationCount: 0,
    openPrCount: 0,
    openPrNumber: null,
    openPrUrl: null,
    openPrStatus: null,
    currentUserAssigned: false,
    currentUserWatching: false,
  };
}

function PullRequestAssociationItem({
  icon,
  title,
  subtitle,
  onClick,
}: {
  icon: ReactNode;
  title: string;
  subtitle?: string | null | undefined;
  onClick?: (() => void) | undefined;
}) {
  const content = (
    <>
      <div className="flex items-center justify-between gap-2">
        <span className="flex min-w-0 items-center gap-1.5">
          {icon}
          <span className="truncate text-sm font-medium">{title}</span>
        </span>
        {onClick ? <ChevronRight className="h-3.5 w-3.5 shrink-0" aria-hidden="true" /> : null}
      </div>
      {subtitle ? <p className="mt-1 truncate text-xs text-[var(--text-muted)]">{subtitle}</p> : null}
    </>
  );
  const className = "w-full rounded-md px-3 py-2 text-left";
  const style = {
    backgroundColor: "var(--bg-elevated)",
    borderColor: "var(--border-subtle)",
    borderStyle: "solid",
    borderWidth: "1px",
    color: "var(--text-primary)",
  } as const;

  if (!onClick) {
    return (
      <div className={className} style={style}>
        {content}
      </div>
    );
  }
  return (
    <button
      type="button"
      className={`${className} hover:bg-[var(--bg-hover)] focus-visible:outline-none focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]`}
      style={style}
      onClick={onClick}
    >
      {content}
    </button>
  );
}

function PullRequestAssociationSection({
  title,
  count,
  children,
}: {
  title: string;
  count: number;
  children: ReactNode;
}) {
  return (
    <section>
      <h4 className="mb-2 text-[11px] font-semibold uppercase text-[var(--text-muted)]">
        {title} ({count})
      </h4>
      <div className="space-y-2">{children}</div>
    </section>
  );
}

function PullRequestAssociationEmptyState({
  description,
  actionLabel,
  onAction,
}: {
  description: string;
  actionLabel: string;
  onAction?: (() => void) | undefined;
}) {
  return (
    <div className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-elevated)] px-3 py-2">
      <p className="text-sm text-[var(--text-muted)]">{description}</p>
      <Button
        type="button"
        variant="ghost"
        size="sm"
        className="mt-2 h-7 w-full justify-center text-xs"
        disabled={!onAction}
        onClick={onAction}
      >
        {actionLabel}
      </Button>
    </div>
  );
}

function PullRequestGranolaBindControl({
  projectId,
  notes,
  conversations,
}: {
  projectId: string;
  notes: GranolaNoteSummary[];
  conversations: PullRequestRxConversation[];
}) {
  const queryClient = useQueryClient();
  const [open, setOpen] = useState(false);
  const [selectedNoteId, setSelectedNoteId] = useState("");
  const [selectedConversationId, setSelectedConversationId] = useState("");
  const selectedNote = notes.find((note) => note.id === selectedNoteId) ?? null;
  const canBind = Boolean(projectId && selectedNote && selectedConversationId);
  const bindMutation = useMutation({
    mutationFn: async () => {
      if (!selectedNote || !selectedConversationId) {
        throw new Error("Select a Granola note and RalphX conversation.");
      }
      return granolaApi.assignAgentConversationGranolaNote({
        conversationId: selectedConversationId,
        projectId,
        noteId: selectedNote.id,
        title: selectedNote.title ?? null,
        noteUrl: selectedNote.url ?? null,
        summary: selectedNote.summary ?? null,
        includeTranscript: true,
        refresh: true,
      });
    },
    onSuccess: (_note, _variables, _context) => {
      void invalidateAgentConversationGranolaNote(queryClient, selectedConversationId);
      void queryClient.invalidateQueries({
        queryKey: granolaDashboardKeys.notes(projectId),
      });
      setOpen(false);
      setSelectedNoteId("");
    },
  });
  const bindError =
    bindMutation.error instanceof Error
      ? bindMutation.error.message
      : bindMutation.error
        ? "Granola note could not be bound."
        : null;

  useEffect(() => {
    if (!open) {
      return;
    }
    if (
      selectedConversationId &&
      conversations.some((conversation) => conversation.conversationId === selectedConversationId)
    ) {
      return;
    }
    setSelectedConversationId(conversations[0]?.conversationId ?? "");
  }, [conversations, open, selectedConversationId]);

  useEffect(() => {
    if (!selectedNoteId) {
      return;
    }
    if (notes.some((note) => note.id === selectedNoteId)) {
      return;
    }
    setSelectedNoteId("");
  }, [notes, selectedNoteId]);

  return (
    <div className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-elevated)] p-2">
      <Button
        type="button"
        variant="ghost"
        size="sm"
        className="h-7 w-full justify-center text-xs"
        disabled={conversations.length === 0 || notes.length === 0}
        onClick={() => setOpen((current) => !current)}
        aria-expanded={open}
      >
        Add Granola
      </Button>
      {open ? (
        <div className="mt-2 grid gap-2">
          <TicketSearchableSelect
            ariaLabel="Granola note"
            value={selectedNoteId}
            onValueChange={setSelectedNoteId}
            placeholder={notes.length === 0 ? "No notes" : "Select note"}
            searchPlaceholder="Search Granola notes..."
            emptyLabel="No Granola notes found"
            disabled={notes.length === 0}
            options={notes.map((note) => ({
              value: note.id,
              label: note.title ?? note.id,
              description: prGranolaNoteSubtitle(note) ?? note.id,
            }))}
          />
          <TicketSearchableSelect
            ariaLabel="Granola conversation"
            value={selectedConversationId}
            onValueChange={setSelectedConversationId}
            placeholder={conversations.length === 0 ? "No conversations" : "Select conversation"}
            searchPlaceholder="Search conversations..."
            emptyLabel="No conversations found"
            disabled={conversations.length === 0}
            options={conversations.map((conversation) => ({
              value: conversation.conversationId,
              label: conversation.branchName || "RalphX conversation",
              description: conversation.conversationId,
            }))}
          />
          {bindError ? (
            <p className="text-xs text-[var(--status-error)]">{bindError}</p>
          ) : null}
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="h-7 w-full justify-center text-xs"
            disabled={!canBind || bindMutation.isPending}
            onClick={() => bindMutation.mutate()}
          >
            {bindMutation.isPending ? "Binding..." : "Bind Granola note"}
          </Button>
        </div>
      ) : null}
      {conversations.length === 0 ? (
        <p className="mt-2 text-xs leading-5 text-[var(--text-muted)]">
          Attach a RalphX conversation before binding Granola.
        </p>
      ) : notes.length === 0 ? (
        <p className="mt-2 text-xs leading-5 text-[var(--text-muted)]">
          No unlinked Granola notes available.
        </p>
      ) : null}
    </div>
  );
}

function PullRequestAssociationRail({
  detail,
  shell,
  projectId,
  loading,
  granolaEnabled,
  granolaLoading,
  allGranolaNotes,
  onNavigateToAssociation,
  onOpenTicket,
  onOpenGranolaNote,
}: {
  detail: PullRequestDetail | null;
  shell: PullRequestShell | null;
  projectId: string;
  loading: boolean;
  granolaEnabled: boolean;
  granolaLoading: boolean;
  allGranolaNotes: GranolaNoteSummary[];
  onNavigateToAssociation?: ((deepLink: TicketDeepLink) => void) | undefined;
  onOpenTicket?: ((ticket: PullRequestTicketSelection) => void) | undefined;
  onOpenGranolaNote?: ((noteId: string) => void) | undefined;
}) {
  const detailRxConversations = detail?.rxConversations;
  const shellRxConversations = shell?.rxConversations;
  const shellBranch = shell?.branch;
  const shellConversationId = shell?.conversationId;
  const shellPrNumber = shell?.prNumber;
  const shellStatus = shell?.status;
  const detailLinkedTickets = detail?.linkedTickets;
  const shellTicketLinks = shell?.ticketLinks;
  const rxConversations = useMemo<PullRequestRxConversation[]>(() => {
    if (detailRxConversations?.length) {
      return detailRxConversations;
    }
    if (shellRxConversations?.length) {
      return shellRxConversations.map((conversation) => ({
        conversationId: conversation.conversationId,
        branchName: conversation.title ?? shellBranch ?? "",
        linkedIdeationSessionId: null,
        publicationPrNumber: shellPrNumber ?? null,
        publicationPrStatus: shellStatus ?? null,
      }));
    }
    if (shellConversationId) {
      return [{
        conversationId: shellConversationId,
        branchName: shellBranch ?? "",
        linkedIdeationSessionId: null,
        publicationPrNumber: shellPrNumber ?? null,
        publicationPrStatus: shellStatus ?? null,
      }];
    }
    return [];
  }, [
    detailRxConversations,
    shellBranch,
    shellConversationId,
    shellPrNumber,
    shellRxConversations,
    shellStatus,
  ]);
  const linkedTickets = useMemo(
    () =>
      [
        ...(detailLinkedTickets ?? []).map((ticket) => ({
          provider: ticket.provider,
          issueKey: ticket.issueKey,
          title: null,
          url: ticket.url ?? null,
        })),
        ...(shellTicketLinks ?? []).map((ticket) => ({
          provider: ticket.provider,
          issueKey: ticket.label,
          title: ticket.title,
          url: ticket.url ?? null,
        })),
      ].reduce<PullRequestTicketAssociation[]>((tickets, ticket) => {
        const existing = tickets.find((candidate) =>
          candidate.provider === ticket.provider && candidate.issueKey === ticket.issueKey,
        );
        if (existing) {
          existing.title = existing.title ?? ticket.title;
          existing.url = existing.url ?? ticket.url;
          return tickets;
        }
        tickets.push(ticket);
        return tickets;
      }, []),
    [detailLinkedTickets, shellTicketLinks],
  );
  const prNumber = detail?.description?.number ?? shell?.prNumber ?? null;
  const granolaNotes = useMemo(
    () =>
      allGranolaNotes.filter((note) =>
        granolaNoteMatchesPullRequest({
          note,
          prNumber,
          rxConversations,
          linkedTickets,
        }),
      ),
    [allGranolaNotes, linkedTickets, prNumber, rxConversations],
  );
  const linkedGranolaNoteIds = new Set(granolaNotes.map((note) => note.id));
  const availableGranolaNotes = allGranolaNotes.filter((note) => !linkedGranolaNoteIds.has(note.id));

  return (
    <aside
      className="flex min-h-0 flex-col p-4"
      style={{
        backgroundColor: "var(--bg-surface)",
        borderLeftColor: "var(--border-subtle)",
        borderLeftStyle: "solid",
        borderLeftWidth: "1px",
      }}
    >
      <div className="flex items-center gap-2">
        <GitPullRequestArrow className="h-4 w-4 text-[var(--text-muted)]" aria-hidden="true" />
        <h3 className="text-sm font-semibold text-[var(--text-primary)]">Associations</h3>
      </div>
      {loading ? (
        <p className="mt-4 text-sm text-[var(--text-muted)]">Loading associations</p>
      ) : (
        <div className="mt-4 min-h-0 space-y-4 overflow-auto">
          <PullRequestAssociationSection title="RX Conversations" count={rxConversations.length}>
            {rxConversations.length > 0 ? (
              rxConversations.map((conversation) => (
                <PullRequestAssociationItem
                  key={conversation.conversationId}
                  icon={<Briefcase className="h-3.5 w-3.5 shrink-0" aria-hidden="true" />}
                  title={conversation.branchName || "RalphX conversation"}
                  subtitle={conversation.conversationId}
                  onClick={
                    onNavigateToAssociation
                      ? () => onNavigateToAssociation({
                          view: "agents",
                          id: conversation.conversationId,
                          projectId,
                        })
                      : undefined
                  }
                />
              ))
            ) : (
              <PullRequestAssociationEmptyState
                description="No RalphX conversation attached."
                actionLabel="Open Agents"
                onAction={
                  onNavigateToAssociation
                    ? () => onNavigateToAssociation({ view: "agents", id: "", projectId })
                    : undefined
                }
              />
            )}
          </PullRequestAssociationSection>

          <PullRequestAssociationSection title="Tickets" count={linkedTickets.length}>
            {linkedTickets.length > 0 ? (
              linkedTickets.map((ticket) => {
                const fallbackTicket = prTicketFallbackSummary(ticket);
                return (
                  <PullRequestAssociationItem
                    key={`${ticket.provider}:${ticket.issueKey}`}
                    icon={<Ticket className="h-3.5 w-3.5 shrink-0" aria-hidden="true" />}
                    title={`${prTicketProviderLabel(ticket.provider)} ${ticket.issueKey}`}
                    subtitle={ticket.title ?? ticket.url}
                    onClick={
                      fallbackTicket && onOpenTicket
                        ? () => onOpenTicket({
                            ticketRef: fallbackTicket.ref,
                            fallbackTicket,
                          })
                        : undefined
                    }
                  />
                );
              })
            ) : (
              <PullRequestAssociationEmptyState
                description="No ticket attached."
                actionLabel="Open Ticketing"
                onAction={
                  onNavigateToAssociation
                    ? () => onNavigateToAssociation({ view: "ticketing", id: "", projectId })
                    : undefined
                }
              />
            )}
          </PullRequestAssociationSection>

          {granolaEnabled ? (
            <PullRequestAssociationSection title="Granola" count={granolaNotes.length}>
              {granolaLoading ? (
                <p className="text-sm text-[var(--text-muted)]">Loading Granola notes</p>
              ) : (
                <>
                  <PullRequestGranolaBindControl
                    projectId={projectId}
                    notes={availableGranolaNotes}
                    conversations={rxConversations}
                  />
                  {granolaNotes.length > 0 ? (
                    granolaNotes.map((note) => (
                      <PullRequestAssociationItem
                        key={note.id}
                        icon={<ScrollText className="h-3.5 w-3.5 shrink-0" aria-hidden="true" />}
                        title={note.title ?? note.id}
                        subtitle={prGranolaNoteSubtitle(note)}
                        onClick={
                          onOpenGranolaNote
                            ? () => onOpenGranolaNote(note.id)
                            : undefined
                        }
                      />
                    ))
                  ) : (
                    <p className="text-sm text-[var(--text-muted)]">
                      No Granola notes linked.
                    </p>
                  )}
                </>
              )}
            </PullRequestAssociationSection>
          ) : null}
        </div>
      )}
    </aside>
  );
}

export function PullRequestDetailSheet({
  open,
  selector,
  shell,
  onNavigateToAssociation,
  onClose,
}: {
  open: boolean;
  selector: PullRequestDetailSelector | null;
  shell: PullRequestShell | null;
  onNavigateToAssociation?: ((deepLink: TicketDeepLink) => void) | undefined;
  onClose: () => void;
}) {
  const [selectedTicket, setSelectedTicket] =
    useState<PullRequestTicketSelection | null>(null);
  const shouldFetchDetail = useAfterPaint(open && Boolean(selector));
  const detailQuery = usePullRequestDetail(selector, { enabled: shouldFetchDetail });
  const detail = detailQuery.data ?? null;
  const projectId = shell?.projectId ?? selector?.projectId ?? "";
  const shouldFetchGranola = useAfterPaint(open && Boolean(projectId));
  const granolaSettingsQuery = useQuery({
    queryKey: granolaDashboardKeys.settings(),
    queryFn: () => granolaApi.getSettings(),
    enabled: shouldFetchGranola,
    staleTime: 30_000,
  });
  const granolaEnabled =
    granolaSettingsQuery.data?.enabled === true &&
    granolaSettingsQuery.data.hasApiToken === true &&
    granolaSettingsQuery.data.validationStatus === "valid";
  const granolaNotesQuery = useQuery({
    queryKey: granolaDashboardKeys.notes(projectId),
    queryFn: () => granolaApi.listNotes({ pageSize: 100, projectId }),
    enabled: shouldFetchGranola && granolaEnabled && Boolean(projectId),
    staleTime: 20_000,
  });
  const loading =
    detailQuery.isLoading ||
    (shouldFetchDetail && !detail && detailQuery.fetchStatus !== "idle");
  const ticketSheetReady = useAfterPaint(Boolean(selectedTicket));
  const ticketDetailInput = selectedTicket && ticketSheetReady
    ? { provider: selectedTicket.ticketRef.provider, ticketRef: selectedTicket.ticketRef }
    : null;
  const ticketDetailQuery = useTicketDetail(ticketDetailInput, {
    enabled: Boolean(ticketDetailInput),
  });
  const ticketTransitionsQuery = useTicketTransitions(ticketDetailInput, {
    enabled: Boolean(ticketDetailInput),
  });
  const ticketAssociationsQuery = useTicketAssociations(
    ticketDetailInput ? { ...ticketDetailInput, projectId } : null,
    { enabled: Boolean(ticketDetailInput && projectId) },
  );
  const ticketForSheet = ticketDetailQuery.data ?? selectedTicket?.fallbackTicket ?? null;

  useEffect(() => {
    setSelectedTicket(null);
  }, [selector?.projectId, selector?.prNumber, selector?.branch, shell?.branch]);

  function handleOpenGranolaNote(noteId: string) {
    if (!projectId) {
      return;
    }
    onNavigateToAssociation?.({ view: "granola", id: noteId, projectId });
  }

  return (
    <>
      <Dialog
        open={open}
        onOpenChange={(nextOpen) => {
          if (!nextOpen) {
            onClose();
          }
        }}
      >
        <DialogContent
          hideCloseButton
          className="left-auto right-0 top-12 h-[calc(100vh-3rem)] w-[64vw] min-w-[820px] max-w-[1180px] translate-x-0 translate-y-0 rounded-none p-0"
          style={{
            backgroundColor: "var(--bg-elevated)",
            borderColor: "var(--border-subtle)",
            borderStyle: "solid",
            borderWidth: "1px",
            boxShadow: "var(--shadow-lg)",
          }}
        >
          <div className="grid h-full min-h-0 grid-cols-[minmax(0,1fr)_320px]">
            <div className="flex min-h-0 flex-col">
              <DialogHeader className="shrink-0 px-5 py-4">
                <div className="min-w-0">
                  <DialogTitle className="truncate text-base">
                    {shell?.prNumber ? `PR #${shell.prNumber}` : "Pull request"}
                  </DialogTitle>
                  <DialogDescription className="mt-1 truncate">
                    {shell?.title ?? shell?.branch ?? "GitHub pull request"}
                  </DialogDescription>
                </div>
                <Button type="button" variant="ghost" size="sm" onClick={onClose}>
                  <X className="h-4 w-4" aria-hidden="true" />
                  Close
                </Button>
              </DialogHeader>
              <div className="min-h-0 flex-1 overflow-auto">
                <PullRequestDetailBody selector={selector} shell={shell} />
              </div>
            </div>
            <PullRequestAssociationRail
              detail={detail}
              shell={shell}
              projectId={projectId}
              loading={loading}
              granolaEnabled={granolaEnabled}
              granolaLoading={granolaNotesQuery.isLoading || granolaNotesQuery.isFetching}
              allGranolaNotes={granolaNotesQuery.data?.notes ?? []}
              onNavigateToAssociation={onNavigateToAssociation}
              onOpenTicket={setSelectedTicket}
              onOpenGranolaNote={handleOpenGranolaNote}
            />
          </div>
        </DialogContent>
      </Dialog>
      <TicketDetailSheet
        open={selectedTicket !== null}
        ticket={ticketForSheet}
        capabilities={null}
        transitions={ticketTransitionsQuery.data ?? []}
        associations={ticketAssociationsQuery.data}
        projectId={projectId}
        isDetailLoading={ticketDetailQuery.isLoading || ticketDetailQuery.isFetching}
        isAssociationsLoading={ticketAssociationsQuery.isLoading}
        isTransitionPending={false}
        isAssignPending={false}
        isCommentPending={false}
        showStartWork={false}
        showConversationBinding={false}
        onNavigate={onNavigateToAssociation}
        onClose={() => setSelectedTicket(null)}
      />
    </>
  );
}
