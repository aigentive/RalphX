import { useEffect, useMemo, useState, type CSSProperties } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Briefcase,
  CalendarClock,
  Check,
  ChevronRight,
  Copy,
  GitPullRequest,
  Loader2,
  RefreshCw,
  ScrollText,
  Search,
  Ticket,
  X,
} from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

import { githubApi } from "@/api/github";
import {
  granolaApi,
  type GranolaNoteDetail,
  type GranolaNoteSummary,
  type GranolaNoteTicketLink,
} from "@/api/granola";
import { atlassianApi } from "@/api/atlassian";
import { linearApi } from "@/api/linear";
import type {
  TicketDeepLink,
  TicketingProvider,
  TicketingProviderSummary,
  TicketRef,
  TicketSummary,
} from "@/api/ticketing";
import { ticketingApi } from "@/api/ticketing";
import { invalidateAgentConversationGranolaNote } from "@/components/agents/agentGranolaNoteQueries";
import { markdownComponents } from "@/components/Chat/MessageItem.markdown";
import { githubBranchOverviewKeys } from "@/components/github/githubBranchOverviewKeys";
import { TicketDetailSheet } from "@/components/ticketing/TicketDetailSheet";
import { TicketSearchableSelect } from "@/components/ticketing/TicketSearchableSelect";
import { TicketingStatePanel } from "@/components/ticketing/TicketingStatePanel";
import { useAfterPaint } from "@/components/ticketing/useAfterPaint";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  useTicketAssociations,
  useTicketDetail,
  useTicketTransitions,
  ticketingKeys,
} from "@/hooks/useTicketing";
import {
  DEFAULT_GRANOLA_DASHBOARD_STATE,
  useIntegrationDashboardStore,
  type GranolaDashboardNoteFilter,
} from "@/stores/integrationDashboardStore";
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
import {
  getProjectRepositoryCapability,
  isGithubRepositoryCapability,
  type Project,
} from "@/types/project";

import { GranolaIcon } from "./GranolaIcon";
import { granolaDashboardKeys } from "./granolaDashboardKeys";

type GranolaNoteFilter = GranolaDashboardNoteFilter;
type GranolaDateGroup = "today" | "yesterday" | "this_week" | "older" | "undated";
type GranolaTicketSelection = {
  ticketRef: TicketRef;
  fallbackTicket: TicketSummary;
};
type GranolaExistingTicketOption = {
  value: string;
  ticket: TicketSummary;
};
type GranolaTicketConversationBindInput = {
  conversationId: string;
  ticket: TicketSummary;
};
type GranolaPrConversationOption = {
  conversationId: string;
  label: string;
  description: string;
  prNumber: number;
  branchName: string;
};

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
  { id: "with_rx", label: "RX" },
  { id: "with_tickets", label: "Tickets" },
  { id: "with_prs", label: "PRs" },
];
const TICKET_BINDING_PROVIDERS: TicketingProvider[] = ["linear", "jira", "clickup"];

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
  const haystack = [
    note.title,
    note.summary,
    note.id,
    note.url,
    ...(note.rxConversations ?? []).flatMap((conversation) => [
      conversation.title,
      conversation.conversationId,
    ]),
    ...(note.ticketLinks ?? []).flatMap((ticketLink) => [
      ticketLink.provider,
      ticketLink.label,
      ticketLink.title,
      ticketLink.url,
    ]),
    ...(note.pullRequests ?? []).flatMap((pullRequest) => [
      `#${pullRequest.number}`,
      pullRequest.status,
      pullRequest.url,
    ]),
  ]
    .filter(Boolean)
    .join(" ")
    .toLowerCase();
  return haystack.includes(trimmed);
}

function granolaNoteRxCount(note: GranolaNoteSummary): number {
  return note.rxConversationCount ?? note.rxConversations?.length ?? 0;
}

function granolaNoteTicketCount(note: GranolaNoteSummary): number {
  return note.ticketCount ?? note.ticketLinks?.length ?? 0;
}

function granolaNotePrCount(note: GranolaNoteSummary): number {
  return note.prCount ?? note.pullRequests?.length ?? 0;
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
    case "with_rx":
      return granolaNoteRxCount(note) > 0;
    case "with_tickets":
      return granolaNoteTicketCount(note) > 0;
    case "with_prs":
      return granolaNotePrCount(note) > 0;
    case "all":
      return true;
  }
}

function noteFilterCount(notes: GranolaNoteSummary[], filter: GranolaNoteFilter): number {
  return notes.filter((note) => granolaNoteMatchesFilter(note, filter)).length;
}

function granolaTicketProviderLabel(provider: string): string {
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

function canBindGranolaTicketProvider(
  provider: TicketingProvider,
): provider is "jira" | "linear" {
  return provider === "jira" || provider === "linear";
}

function granolaTicketOptionValue(ticket: TicketSummary): string {
  return `${ticket.ref.provider}:${ticket.ref.id}:${ticket.ref.key ?? ""}`;
}

function granolaTicketOptionDescription(ticket: TicketSummary): string {
  return [
    ticket.ref.key ?? ticket.ref.id,
    ticket.state.name,
    ticket.project,
  ].filter(Boolean).join(" - ");
}

function sortedBindableTicketingProviders(
  providers: TicketingProviderSummary[],
): TicketingProviderSummary[] {
  return providers
    .filter((provider) => provider.enabled && provider.connectionStatus === "connected")
    .sort((left, right) => {
      const leftIndex = TICKET_BINDING_PROVIDERS.indexOf(left.provider);
      const rightIndex = TICKET_BINDING_PROVIDERS.indexOf(right.provider);
      return leftIndex - rightIndex;
    });
}

function granolaTicketProvider(provider: string): TicketingProvider | null {
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

function granolaTicketRef(
  ticketLink: GranolaNoteTicketLink,
): TicketRef | null {
  const provider = granolaTicketProvider(ticketLink.provider);
  if (!provider) {
    return null;
  }
  return {
    provider,
    id: ticketLink.label,
    key: ticketLink.label,
  };
}

function granolaTicketFallbackSummary(
  ticketLink: GranolaNoteTicketLink,
  note: GranolaNoteSummary,
): TicketSummary | null {
  const ticketRef = granolaTicketRef(ticketLink);
  if (!ticketRef) {
    return null;
  }
  return {
    ref: ticketRef,
    title: ticketLink.title ?? ticketLink.label,
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
    updatedAt: granolaNoteTimestamp(note) ?? new Date(0).toISOString(),
    url: ticketLink.url ?? null,
    associationCount: 0,
    openPrCount: 0,
    openPrNumber: null,
    openPrUrl: null,
    openPrStatus: null,
    currentUserAssigned: false,
    currentUserWatching: false,
  };
}

function granolaAssociationCount(note: GranolaNoteSummary | null | undefined): number {
  if (!note) {
    return 0;
  }
  return (
    granolaNoteRxCount(note)
    + granolaNoteTicketCount(note)
    + granolaNotePrCount(note)
  );
}

function GranolaAssociationPills({
  note,
  compact = false,
}: {
  note: GranolaNoteSummary | null | undefined;
  compact?: boolean;
}) {
  if (!note || granolaAssociationCount(note) === 0) {
    return null;
  }

  const rxCount = granolaNoteRxCount(note);
  const ticketCount = granolaNoteTicketCount(note);
  const prCount = granolaNotePrCount(note);
  const items = [
    {
      id: "rx",
      count: rxCount,
      label: compact ? String(rxCount) : `${rxCount} RX`,
      ariaLabel: `${rxCount} RalphX conversation${rxCount === 1 ? "" : "s"} attached`,
      icon: Briefcase,
    },
    {
      id: "tickets",
      count: ticketCount,
      label: compact
        ? String(ticketCount)
        : `${ticketCount} ticket${ticketCount === 1 ? "" : "s"}`,
      ariaLabel: `${ticketCount} ticket${ticketCount === 1 ? "" : "s"} attached`,
      icon: Ticket,
    },
    {
      id: "prs",
      count: prCount,
      label: compact ? String(prCount) : `${prCount} PR${prCount === 1 ? "" : "s"}`,
      ariaLabel: `${prCount} pull request${prCount === 1 ? "" : "s"} attached`,
      icon: GitPullRequest,
    },
  ].filter((item) => item.count > 0);

  return (
    <span
      className={cn(
        "inline-flex items-center gap-1",
        compact ? "shrink-0 flex-nowrap" : "flex-wrap",
      )}
    >
      {items.map(({ id, label, ariaLabel, icon: Icon }) => (
        <span
          key={id}
          aria-label={ariaLabel}
          className={cn(
            "inline-flex shrink-0 items-center rounded-full border text-xs font-medium",
            compact ? "h-5 gap-1 px-1.5" : "h-6 gap-1.5 px-2",
          )}
          style={{
            backgroundColor: "var(--bg-elevated)",
            borderColor: "var(--border-subtle)",
            borderStyle: "solid",
            borderWidth: "1px",
            color: "var(--text-muted)",
          }}
        >
          <Icon className={compact ? "h-3 w-3" : "h-3.5 w-3.5"} aria-hidden="true" />
          <span>{label}</span>
        </span>
      ))}
    </span>
  );
}

function GranolaAssociationItem({
  icon,
  title,
  subtitle,
  onClick,
}: {
  icon: React.ReactNode;
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

function GranolaAssociationSection({
  title,
  count,
  children,
}: {
  title: string;
  count: number;
  children: React.ReactNode;
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

function GranolaAssociationEmptyState({
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

function GranolaAssociationRail({
  note,
  projectId,
  onNavigateToAssociation,
  onOpenTicket,
  onAddContext,
}: {
  note: GranolaNoteSummary | null | undefined;
  projectId: string;
  onNavigateToAssociation?: ((deepLink: TicketDeepLink) => void) | undefined;
  onOpenTicket?: ((ticket: GranolaTicketSelection) => void) | undefined;
  onAddContext?: (() => void) | undefined;
}) {
  if (!note) {
    return null;
  }

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
      <div className="flex items-center justify-between gap-3">
        <div className="flex items-center gap-2">
          <ScrollText className="h-4 w-4 text-[var(--text-muted)]" aria-hidden="true" />
          <h3 className="text-sm font-semibold text-[var(--text-primary)]">Associations</h3>
        </div>
        <GranolaAssociationPills note={note} compact />
      </div>
      <div className="mt-3 grid gap-2">
        <Button type="button" variant="outline" size="sm" onClick={onAddContext}>
          <Briefcase className="h-3.5 w-3.5" aria-hidden="true" />
          Add context
        </Button>
        <p className="text-xs leading-5 text-[var(--text-muted)]">
          Tickets and PRs are linked through RalphX conversations.
        </p>
      </div>

      <div className="mt-4 min-h-0 space-y-4 overflow-auto">
        <GranolaAssociationSection
          title="RX Conversations"
          count={note.rxConversations?.length ?? 0}
        >
          {(note.rxConversations ?? []).length > 0 ? (
            (note.rxConversations ?? []).map((conversation) => (
              <GranolaAssociationItem
                key={conversation.conversationId}
                icon={<Briefcase className="h-3.5 w-3.5 shrink-0" aria-hidden="true" />}
                title={conversation.title ?? "RalphX conversation"}
                subtitle={conversation.conversationId}
                onClick={() => {
                  onNavigateToAssociation?.({
                    view: "agents",
                    id: conversation.conversationId,
                    projectId,
                  });
                }}
              />
            ))
          ) : (
            <GranolaAssociationEmptyState
              description="No RalphX conversation attached."
              actionLabel="Add context"
              onAction={onAddContext}
            />
          )}
        </GranolaAssociationSection>

        <GranolaAssociationSection title="Tickets" count={note.ticketLinks?.length ?? 0}>
          {(note.ticketLinks ?? []).length > 0 ? (
            (note.ticketLinks ?? []).map((ticketLink) => {
              const ticketLabel = `${granolaTicketProviderLabel(ticketLink.provider)} ${ticketLink.label}`;
              const fallbackTicket = granolaTicketFallbackSummary(ticketLink, note);
              return (
                <GranolaAssociationItem
                  key={`${ticketLink.provider}:${ticketLink.label}`}
                  icon={<Ticket className="h-3.5 w-3.5 shrink-0" aria-hidden="true" />}
                  title={ticketLabel}
                  subtitle={ticketLink.title ?? ticketLink.url}
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
            <GranolaAssociationEmptyState
              description="No ticket linked."
              actionLabel="Open Ticketing"
              onAction={() => onNavigateToAssociation?.({ view: "ticketing", id: "", projectId })}
            />
          )}
        </GranolaAssociationSection>

        <GranolaAssociationSection title="Pull Requests" count={note.pullRequests?.length ?? 0}>
          {(note.pullRequests ?? []).length > 0 ? (
            (note.pullRequests ?? []).map((pullRequest) => (
              <GranolaAssociationItem
                key={pullRequest.number}
                icon={<GitPullRequest className="h-3.5 w-3.5 shrink-0" aria-hidden="true" />}
                title={`PR #${pullRequest.number}`}
                subtitle={pullRequest.status ?? pullRequest.url}
                onClick={() => {
                  onNavigateToAssociation?.({
                    view: "github",
                    id: String(pullRequest.number),
                    projectId,
                  });
                }}
              />
            ))
          ) : (
            <GranolaAssociationEmptyState
              description="No pull request linked."
              actionLabel="Open GitHub"
              onAction={() => onNavigateToAssociation?.({ view: "github", id: "", projectId })}
            />
          )}
        </GranolaAssociationSection>
      </div>
    </aside>
  );
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
    <div
      className="theme-aware-prose prose prose-sm max-w-none text-sm leading-6"
      data-testid="granola-dashboard-note-markdown"
    >
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={markdownComponents}>
        {markdown}
      </ReactMarkdown>
    </div>
  );
}

function GranolaNoteDetailSheet({
  open,
  note,
  summaryNote,
  projectId,
  isDetailLoading,
  transcriptText,
  copiedAction,
  onCopy,
  onAddContext,
  onNavigateToAssociation,
  onOpenTicket,
  onClose,
}: {
  open: boolean;
  note: GranolaNoteDetail | GranolaNoteSummary | null;
  summaryNote: GranolaNoteSummary | null;
  projectId: string;
  isDetailLoading: boolean;
  transcriptText: string;
  copiedAction: "summary" | "transcript" | null;
  onCopy: (kind: "summary" | "transcript", text: string) => void;
  onAddContext: () => void;
  onNavigateToAssociation?: ((deepLink: TicketDeepLink) => void) | undefined;
  onOpenTicket: (ticket: GranolaTicketSelection) => void;
  onClose: () => void;
}) {
  const title = note?.title ?? summaryNote?.title ?? note?.id ?? summaryNote?.id ?? "Granola note";
  const timestampSource = note ?? summaryNote;
  const timestamp = timestampSource ? granolaNoteTimestamp(timestampSource) : null;

  return (
    <Dialog open={open} onOpenChange={(nextOpen) => !nextOpen && onClose()}>
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
                <div className="flex items-center gap-2 text-xs font-medium uppercase text-[var(--text-muted)]">
                  <ScrollText className="h-4 w-4" aria-hidden="true" />
                  Granola note
                </div>
                <DialogTitle className="mt-1 truncate text-base">
                  {title}
                </DialogTitle>
                <DialogDescription className="mt-1 truncate">
                  {timestamp
                    ? [
                        formatGranolaNoteDate(timestamp),
                        formatGranolaNoteTime(timestamp),
                      ].filter(Boolean).join(" at ")
                    : "Meeting notes and transcript context"}
                </DialogDescription>
              </div>
              <Button type="button" variant="ghost" size="sm" onClick={onClose}>
                <X className="h-4 w-4" aria-hidden="true" />
                Close
              </Button>
            </DialogHeader>

            <div className="min-h-0 flex-1 overflow-auto p-5">
              {!note && !summaryNote ? (
                <div className="flex items-center gap-2 text-sm text-[var(--text-muted)]">
                  <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
                  Loading note
                </div>
              ) : (
                <div className="mx-auto flex max-w-3xl flex-col gap-4">
                  <div className="flex flex-wrap items-center gap-2">
                    <Button type="button" size="sm" onClick={onAddContext}>
                      Add as context
                    </Button>
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      disabled={!note?.summary?.trim()}
                      onClick={() => onCopy("summary", note?.summary ?? "")}
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
                      onClick={() => onCopy("transcript", transcriptText)}
                    >
                      {copiedAction === "transcript" ? (
                        <Check className="mr-2 h-3.5 w-3.5" aria-hidden="true" />
                      ) : (
                        <Copy className="mr-2 h-3.5 w-3.5" aria-hidden="true" />
                      )}
                      {copiedAction === "transcript" ? "Copied" : "Copy full transcript"}
                    </Button>
                  </div>

                  {isDetailLoading ? (
                    <div className="flex items-center gap-2 text-sm text-[var(--text-muted)]">
                      <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
                      Loading details
                    </div>
                  ) : null}

                  {note?.summary ? (
                    <div
                      className="rounded-md border p-4"
                      style={{
                        backgroundColor: "var(--bg-surface)",
                        borderColor: "var(--border-subtle)",
                        borderStyle: "solid",
                        borderWidth: "1px",
                      }}
                    >
                      <GranolaMarkdownBlock markdown={note.summary} />
                    </div>
                  ) : null}

                  {note && "transcript" in note && note.transcript.length > 0 ? (
                    <div className="space-y-2">
                      <h3 className="text-sm font-semibold text-[var(--text-primary)]">Transcript</h3>
                      <div className="space-y-2">
                        {note.transcript.map((entry, index) => (
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
            </div>
          </div>

          <GranolaAssociationRail
            note={summaryNote}
            projectId={projectId}
            onNavigateToAssociation={onNavigateToAssociation}
            onOpenTicket={onOpenTicket}
            onAddContext={onAddContext}
          />
        </div>
      </DialogContent>
    </Dialog>
  );
}

interface GranolaContextDialogProps {
  open: boolean;
  note: GranolaNoteDetail | GranolaNoteSummary | null;
  projects: Project[];
  selectedProjectId: string;
  conversations: { id: string; title: string | null }[];
  selectedConversationId: string;
  prConversationOptions: GranolaPrConversationOption[];
  selectedPrConversationId: string;
  selectedTicketProvider: TicketingProvider | "";
  selectedTicketId: string;
  selectedTicketConversationId: string;
  ticketingProviders: TicketingProviderSummary[];
  ticketOptions: GranolaExistingTicketOption[];
  isConversationsLoading: boolean;
  isPrConversationsLoading: boolean;
  isTicketingProvidersLoading: boolean;
  isTicketsLoading: boolean;
  isBindPending: boolean;
  isTicketBindPending: boolean;
  bindError: string | null;
  ticketBindError: string | null;
  onProjectChange: (projectId: string) => void;
  onConversationChange: (conversationId: string) => void;
  onPrConversationChange: (conversationId: string) => void;
  onTicketProviderChange: (provider: string) => void;
  onTicketChange: (ticketId: string) => void;
  onTicketConversationChange: (conversationId: string) => void;
  onStartNew: () => void;
  onBindExisting: () => void;
  onBindExistingPr: () => void;
  onBindExistingTicket: () => void;
  onClose: () => void;
}

function GranolaContextDialog({
  open,
  note,
  projects,
  selectedProjectId,
  conversations,
  selectedConversationId,
  prConversationOptions,
  selectedPrConversationId,
  selectedTicketProvider,
  selectedTicketId,
  selectedTicketConversationId,
  ticketingProviders,
  ticketOptions,
  isConversationsLoading,
  isPrConversationsLoading,
  isTicketingProvidersLoading,
  isTicketsLoading,
  isBindPending,
  isTicketBindPending,
  bindError,
  ticketBindError,
  onProjectChange,
  onConversationChange,
  onPrConversationChange,
  onTicketProviderChange,
  onTicketChange,
  onTicketConversationChange,
  onStartNew,
  onBindExisting,
  onBindExistingPr,
  onBindExistingTicket,
  onClose,
}: GranolaContextDialogProps) {
  const selectedTicket = ticketOptions.find((option) => option.value === selectedTicketId)?.ticket ?? null;
  const selectedTicketProviderSupportsBinding =
    selectedTicketProvider !== "" && canBindGranolaTicketProvider(selectedTicketProvider);

  return (
    <Dialog open={open} onOpenChange={(nextOpen) => !nextOpen && onClose()}>
      <DialogContent
        className="max-h-[calc(100dvh-3rem)] w-[min(720px,calc(100vw-2rem))] max-w-none grid-rows-[auto_minmax(0,1fr)_auto] overflow-hidden"
        style={{
          backgroundColor: "var(--bg-elevated)",
        }}
      >
        <DialogHeader className="block space-y-1.5 px-6 py-5 pr-14">
          <DialogTitle className="text-lg leading-6">Add Granola Context</DialogTitle>
          <DialogDescription className="line-clamp-2 max-w-[38rem] leading-5">
            {note?.title ?? note?.id ?? "Granola note"}
          </DialogDescription>
        </DialogHeader>

        <div className="min-h-0 overflow-y-auto overscroll-contain px-6">
          <div className="grid gap-0">
            <section className="py-4">
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
            </section>

            <section className="grid gap-3 border-t border-[var(--border-subtle)] py-4">
              <div className="grid gap-3 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-center">
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
                  className="w-full sm:w-auto"
                  onClick={onStartNew}
                  disabled={!note || !selectedProjectId || projects.length === 0}
                >
                  Open composer
                </Button>
              </div>
            </section>

            <section className="grid gap-3 border-t border-[var(--border-subtle)] py-4">
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
                  className="w-full sm:w-auto"
                  disabled={!note || !selectedConversationId || isBindPending}
                  onClick={onBindExisting}
                >
                  {isBindPending ? "Binding..." : "Bind existing conversation"}
                </Button>
              </div>
            </section>

            <section className="grid gap-3 border-t border-[var(--border-subtle)] py-4">
              <label className="grid gap-1.5 text-sm">
                <span className="font-medium text-[var(--text-primary)]">
                  Existing PR conversation
                </span>
                <TicketSearchableSelect
                  ariaLabel="Existing PR conversation"
                  size="md"
                  value={selectedPrConversationId}
                  onValueChange={onPrConversationChange}
                  placeholder={
                    isPrConversationsLoading
                      ? "Loading pull requests"
                      : prConversationOptions.length === 0
                        ? "No PR conversations"
                        : "Select a PR conversation"
                  }
                  searchPlaceholder="Search pull requests..."
                  emptyLabel="No PR conversations found"
                  disabled={isPrConversationsLoading || prConversationOptions.length === 0}
                  options={prConversationOptions.map((option) => ({
                    value: option.conversationId,
                    label: option.label,
                    description: option.description,
                  }))}
                />
              </label>
              <p className="text-xs leading-5 text-[var(--text-muted)]">
                PR binding uses the RalphX conversation already attached to that PR.
              </p>
              <div className="flex justify-end">
                <Button
                  type="button"
                  variant="outline"
                  className="w-full sm:w-auto"
                  disabled={!note || !selectedPrConversationId || isBindPending}
                  onClick={onBindExistingPr}
                >
                  {isBindPending ? "Binding..." : "Bind selected PR"}
                </Button>
              </div>
            </section>

            <section className="grid gap-3 border-t border-[var(--border-subtle)] py-4">
              <div>
                <p className="text-sm font-medium text-[var(--text-primary)]">
                  Existing ticket
                </p>
                <p className="mt-1 text-xs leading-5 text-[var(--text-muted)]">
                  Bind the note and ticket through a RalphX conversation.
                </p>
              </div>
              <div className="grid gap-3 sm:grid-cols-[180px_minmax(0,1fr)]">
                <label className="grid gap-1.5 text-sm">
                  <span className="font-medium text-[var(--text-primary)]">Provider</span>
                  <TicketSearchableSelect
                    ariaLabel="Ticket provider"
                    size="md"
                    value={selectedTicketProvider}
                    onValueChange={onTicketProviderChange}
                    placeholder={
                      isTicketingProvidersLoading
                        ? "Loading providers"
                        : ticketingProviders.length === 0
                          ? "No providers"
                          : "Provider"
                    }
                    searchPlaceholder="Search providers..."
                    emptyLabel="No providers found"
                    disabled={isTicketingProvidersLoading || ticketingProviders.length === 0}
                    options={ticketingProviders.map((provider) => ({
                      value: provider.provider,
                      label: provider.label,
                      description: canBindGranolaTicketProvider(provider.provider)
                        ? "Direct binding"
                        : "Open only",
                    }))}
                  />
                </label>
                <label className="grid gap-1.5 text-sm">
                  <span className="font-medium text-[var(--text-primary)]">Ticket</span>
                  <TicketSearchableSelect
                    ariaLabel="Existing ticket"
                    size="md"
                    value={selectedTicketId}
                    onValueChange={onTicketChange}
                    placeholder={
                      isTicketsLoading
                        ? "Loading tickets"
                        : ticketOptions.length === 0
                          ? "No tickets"
                          : "Select ticket"
                    }
                    searchPlaceholder="Search tickets..."
                    emptyLabel="No tickets found"
                    disabled={!selectedTicketProvider || isTicketsLoading || ticketOptions.length === 0}
                    options={ticketOptions.map(({ value, ticket }) => ({
                      value,
                      label: `${ticket.ref.key ?? ticket.ref.id} ${ticket.title}`,
                      description: granolaTicketOptionDescription(ticket),
                    }))}
                  />
                </label>
              </div>
              <label className="grid gap-1.5 text-sm">
                <span className="font-medium text-[var(--text-primary)]">
                  Link through conversation
                </span>
                <TicketSearchableSelect
                  ariaLabel="Ticket conversation"
                  size="md"
                  value={selectedTicketConversationId}
                  onValueChange={onTicketConversationChange}
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
              {selectedTicketProvider && !selectedTicketProviderSupportsBinding ? (
                <p className="text-xs leading-5 text-[var(--text-muted)]">
                  {granolaTicketProviderLabel(selectedTicketProvider)} tickets can be opened from
                  associations, but direct conversation binding is not available yet.
                </p>
              ) : null}
              {ticketBindError ? (
                <p className="text-xs text-[var(--status-error)]">{ticketBindError}</p>
              ) : null}
              <div className="flex justify-end">
                <Button
                  type="button"
                  variant="outline"
                  className="w-full sm:w-auto"
                  disabled={
                    !note
                    || !selectedTicket
                    || !selectedTicketConversationId
                    || !selectedTicketProviderSupportsBinding
                    || isTicketBindPending
                  }
                  onClick={onBindExistingTicket}
                >
                  {isTicketBindPending ? "Binding..." : "Bind selected ticket"}
                </Button>
              </div>
            </section>

            <section className="border-t border-[var(--border-subtle)] py-4">
              <div
                className="rounded-md p-3 text-xs leading-5 text-[var(--text-muted)]"
                style={{
                  backgroundColor: "var(--bg-surface)",
                  borderColor: "var(--border-subtle)",
                  borderStyle: "solid",
                  borderWidth: "1px",
                }}
              >
                Existing ticket and PR links are stored through RalphX conversations, so a note can
                appear beside every ticket or pull request attached to the same conversation.
              </div>
            </section>
          </div>
        </div>

        <DialogFooter className="shrink-0 px-6 py-4">
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
          <span className="min-w-0 flex-1 truncate text-sm font-medium">
            {note.title ?? note.id}
          </span>
          <GranolaAssociationPills note={note} compact />
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
  onNavigateToAssociation,
}: {
  projectId: string;
  project: Project | null;
  projects: Project[];
  onStartConversation: (note: GranolaNoteDetail | GranolaNoteSummary, projectId: string) => void;
  onNavigateToAssociation?: ((deepLink: TicketDeepLink) => void) | undefined;
}) {
  const persistedState = useIntegrationDashboardStore((state) => state.granolaByProject[projectId]);
  const setGranolaState = useIntegrationDashboardStore((state) => state.setGranolaState);
  const resetGranolaFilters = useIntegrationDashboardStore((state) => state.resetGranolaFilters);
  const query = persistedState?.query ?? DEFAULT_GRANOLA_DASHBOARD_STATE.query;
  const noteFilter = persistedState?.noteFilter ?? DEFAULT_GRANOLA_DASHBOARD_STATE.noteFilter;
  const selectedNoteId =
    persistedState?.selectedNoteId ?? DEFAULT_GRANOLA_DASHBOARD_STATE.selectedNoteId;
  const [copiedAction, setCopiedAction] = useState<"summary" | "transcript" | null>(null);
  const [contextDialogOpen, setContextDialogOpen] = useState(false);
  const [contextProjectId, setContextProjectId] = useState(projectId);
  const [selectedConversationId, setSelectedConversationId] = useState("");
  const [selectedPrConversationId, setSelectedPrConversationId] = useState("");
  const [selectedTicketProvider, setSelectedTicketProvider] =
    useState<TicketingProvider | "">("");
  const [selectedTicketId, setSelectedTicketId] = useState("");
  const [selectedTicketConversationId, setSelectedTicketConversationId] = useState("");
  const [selectedTicket, setSelectedTicket] = useState<GranolaTicketSelection | null>(null);
  const queryClient = useQueryClient();
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
    { enabled: Boolean(ticketDetailInput) },
  );
  const ticketForSheet = ticketDetailQuery.data ?? selectedTicket?.fallbackTicket ?? null;

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
    queryKey: granolaDashboardKeys.notes(projectId),
    queryFn: () => granolaApi.listNotes({ pageSize: 30, projectId }),
    enabled: granolaReady && Boolean(projectId),
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
  const contextProject =
    project?.id === contextProjectId
      ? project
      : projects.find((candidate) => candidate.id === contextProjectId) ?? null;
  const canLoadGithubOverview = isGithubRepositoryCapability(
    getProjectRepositoryCapability(contextProject),
  );
  const githubOverviewQuery = useQuery({
    queryKey: githubBranchOverviewKeys.project(contextProjectId || projectId),
    queryFn: () => githubApi.getBranchOverview({ projectId: contextProjectId || projectId }),
    enabled:
      contextDialogOpen &&
      Boolean(contextProjectId || projectId) &&
      canLoadGithubOverview,
    staleTime: 15_000,
  });
  const prConversationOptions = useMemo<GranolaPrConversationOption[]>(
    () =>
      (githubOverviewQuery.data?.branches ?? []).flatMap((branch) => {
        if (branch.prNumber == null || branch.rxConversations.length === 0) {
          return [];
        }
        return branch.rxConversations.map((conversation) => ({
          conversationId: conversation.conversationId,
          prNumber: branch.prNumber!,
          branchName: branch.branchName,
          label: `PR #${branch.prNumber} ${branch.prTitle ?? branch.branchName}`,
          description: conversation.title
            ? `${conversation.title} - ${branch.branchName}`
            : branch.branchName,
        }));
      }),
    [githubOverviewQuery.data?.branches],
  );
  const ticketingProvidersQuery = useQuery({
    queryKey: ticketingKeys.providers(contextProjectId || projectId),
    queryFn: () => ticketingApi.listProviders({ projectId: contextProjectId || projectId }),
    enabled: contextDialogOpen && Boolean(contextProjectId || projectId),
    staleTime: 30_000,
  });
  const bindableTicketingProviders = useMemo(
    () => sortedBindableTicketingProviders(ticketingProvidersQuery.data ?? []),
    [ticketingProvidersQuery.data],
  );
  const ticketsForBindingQuery = useQuery({
    queryKey: [
      ...ticketingKeys.all,
      "granola-bind-ticket-options",
      contextProjectId || projectId,
      selectedTicketProvider || null,
    ] as const,
    queryFn: () =>
      ticketingApi.listTickets({
        provider: selectedTicketProvider as TicketingProvider,
        projectId: contextProjectId || projectId,
        limit: 80,
        sort: "updated_desc",
      }),
    enabled: contextDialogOpen && Boolean(contextProjectId || projectId) && Boolean(selectedTicketProvider),
    staleTime: 20_000,
  });
  const ticketOptions = useMemo<GranolaExistingTicketOption[]>(
    () =>
      (ticketsForBindingQuery.data?.items ?? []).map((ticket) => ({
        value: granolaTicketOptionValue(ticket),
        ticket,
      })),
    [ticketsForBindingQuery.data?.items],
  );
  const selectedTicketForBinding =
    ticketOptions.find((option) => option.value === selectedTicketId)?.ticket ?? null;
  const bindGranolaConversation = useMutation({
    mutationFn: async (conversationId: string) => {
      if (!selectedNote || !conversationId) {
        throw new Error("Select a Granola note and conversation.");
      }
      return granolaApi.assignAgentConversationGranolaNote({
        conversationId,
        projectId: contextProjectId || projectId,
        noteId: selectedNote.id,
        title: selectedNote.title ?? null,
        noteUrl: selectedNote.url ?? null,
        summary: selectedNote.summary ?? null,
        includeTranscript: true,
        refresh: true,
      });
    },
    onSuccess: (_note, conversationId) => {
      void invalidateAgentConversationGranolaNote(queryClient, conversationId);
      void queryClient.invalidateQueries({
        queryKey: granolaDashboardKeys.notes(contextProjectId || projectId),
      });
      void queryClient.invalidateQueries({
        queryKey: githubBranchOverviewKeys.project(contextProjectId || projectId),
      });
      setContextDialogOpen(false);
      setSelectedConversationId("");
      setSelectedPrConversationId("");
    },
  });
  const bindTicketToGranolaConversation = useMutation({
    mutationFn: async ({ conversationId, ticket }: GranolaTicketConversationBindInput) => {
      if (!selectedNote || !conversationId) {
        throw new Error("Select a Granola note and conversation.");
      }
      if (!canBindGranolaTicketProvider(ticket.ref.provider)) {
        throw new Error(`${granolaTicketProviderLabel(ticket.ref.provider)} ticket binding is not available yet.`);
      }
      const targetProjectId = contextProjectId || projectId;
      if (ticket.ref.provider === "jira") {
        await atlassianApi.assignAgentConversationJiraIssue({
          conversationId,
          projectId: targetProjectId,
          issueKey: ticket.ref.key ?? ticket.ref.id,
          issueId: ticket.ref.id,
          title: ticket.title,
          issueUrl: ticket.url ?? null,
          refresh: true,
        });
      } else {
        await linearApi.assignAgentConversationLinearIssue({
          conversationId,
          projectId: targetProjectId,
          issueId: ticket.ref.id,
          issueKey: ticket.ref.key ?? null,
          title: ticket.title,
          issueUrl: ticket.url ?? null,
          refresh: true,
        });
      }
      await granolaApi.assignAgentConversationGranolaNote({
        conversationId,
        projectId: targetProjectId,
        noteId: selectedNote.id,
        title: selectedNote.title ?? null,
        noteUrl: selectedNote.url ?? null,
        summary: selectedNote.summary ?? null,
        includeTranscript: true,
        refresh: true,
      });
      return { conversationId, ticket };
    },
    onSuccess: ({ conversationId, ticket }) => {
      const targetProjectId = contextProjectId || projectId;
      void invalidateAgentConversationGranolaNote(queryClient, conversationId);
      void queryClient.invalidateQueries({
        queryKey: granolaDashboardKeys.notes(targetProjectId),
      });
      void queryClient.invalidateQueries({
        queryKey: ticketingKeys.associations({
          provider: ticket.ref.provider,
          ticketRef: ticket.ref,
          projectId: targetProjectId,
        }),
      });
      void queryClient.invalidateQueries({
        queryKey: ticketingKeys.conversationTicket(conversationId),
      });
      void queryClient.invalidateQueries({ queryKey: ticketingKeys.ticketLists() });
      setContextDialogOpen(false);
      setSelectedConversationId("");
      setSelectedPrConversationId("");
      setSelectedTicketId("");
      setSelectedTicketConversationId("");
    },
  });
  const bindError =
    bindGranolaConversation.error instanceof Error
      ? bindGranolaConversation.error.message
      : bindGranolaConversation.error
        ? "Conversation could not be bound."
        : null;
  const ticketBindError =
    bindTicketToGranolaConversation.error instanceof Error
      ? bindTicketToGranolaConversation.error.message
      : bindTicketToGranolaConversation.error
        ? "Ticket could not be bound."
        : null;

  useEffect(() => {
    if (notesQuery.isLoading) {
      return;
    }
    if (!selectedNoteId) {
      return;
    }
    if (filteredNotes.some((note) => note.id === selectedNoteId)) {
      return;
    }
    setGranolaState(projectId, { selectedNoteId: null });
  }, [filteredNotes, notesQuery.isLoading, projectId, selectedNoteId, setGranolaState]);

  useEffect(() => {
    if (!contextDialogOpen) {
      return;
    }
    setContextProjectId(projectId);
    setSelectedConversationId("");
    setSelectedPrConversationId("");
    setSelectedTicketProvider("");
    setSelectedTicketId("");
    setSelectedTicketConversationId("");
  }, [contextDialogOpen, projectId]);

  useEffect(() => {
    if (!contextDialogOpen) {
      return;
    }
    if (
      selectedTicketProvider &&
      bindableTicketingProviders.some((provider) => provider.provider === selectedTicketProvider)
    ) {
      return;
    }
    setSelectedTicketProvider(bindableTicketingProviders[0]?.provider ?? "");
  }, [bindableTicketingProviders, contextDialogOpen, selectedTicketProvider]);

  useEffect(() => {
    if (!contextDialogOpen) {
      return;
    }
    if (
      selectedTicketConversationId &&
      bindableConversations.some((conversation) => conversation.id === selectedTicketConversationId)
    ) {
      return;
    }
    const firstLinkedConversation = selectedSummary?.rxConversations?.[0]?.conversationId;
    const fallbackConversation = bindableConversations.find((conversation) =>
      conversation.id === firstLinkedConversation,
    ) ?? bindableConversations[0];
    setSelectedTicketConversationId(fallbackConversation?.id ?? "");
  }, [bindableConversations, contextDialogOpen, selectedSummary, selectedTicketConversationId]);

  useEffect(() => {
    if (!selectedTicketId) {
      return;
    }
    if (ticketOptions.some((option) => option.value === selectedTicketId)) {
      return;
    }
    setSelectedTicketId("");
  }, [selectedTicketId, ticketOptions]);

  useEffect(() => {
    setSelectedTicket(null);
  }, [projectId]);

  function resetFilters() {
    resetGranolaFilters(projectId);
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

  function handleTicketProviderChange(provider: string) {
    if (provider === "" || TICKET_BINDING_PROVIDERS.includes(provider as TicketingProvider)) {
      setSelectedTicketProvider(provider as TicketingProvider | "");
      setSelectedTicketId("");
    }
  }

  function handleBindExistingTicket() {
    if (!selectedTicketForBinding || !selectedTicketConversationId) {
      return;
    }
    bindTicketToGranolaConversation.mutate({
      conversationId: selectedTicketConversationId,
      ticket: selectedTicketForBinding,
    });
  }

  function handleCopyGranolaText(kind: "summary" | "transcript", text: string) {
    void copyGranolaText(kind, text);
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
              onChange={(event) => setGranolaState(projectId, { query: event.target.value })}
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
                onClick={() => setGranolaState(projectId, { noteFilter: filter.id })}
              >
                {filter.label} {noteFilterCount(notes, filter.id)}
              </GranolaFilterButton>
            ))}
          </div>
        </div>
      </header>

      <div className="grid min-h-0 flex-1 grid-rows-[auto_1fr] overflow-hidden">
        <section
          className="grid min-h-0 grid-rows-[auto_1fr] overflow-hidden"
          aria-label="Granola notes"
          style={{
            backgroundColor: "var(--bg-surface)",
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
                        onSelect={() => setGranolaState(projectId, { selectedNoteId: note.id })}
                      />
                    ))}
                  </section>
                );
              })
            )}
          </div>
        </section>
      </div>

      <GranolaNoteDetailSheet
        open={selectedNoteId !== null}
        note={selectedNote}
        summaryNote={selectedSummary}
        projectId={projectId}
        isDetailLoading={detailQuery.isFetching}
        transcriptText={transcriptText}
        copiedAction={copiedAction}
        onCopy={handleCopyGranolaText}
        onAddContext={() => setContextDialogOpen(true)}
        onNavigateToAssociation={onNavigateToAssociation}
        onOpenTicket={setSelectedTicket}
        onClose={() => setGranolaState(projectId, { selectedNoteId: null })}
      />

      <GranolaContextDialog
        open={contextDialogOpen}
        note={selectedNote}
        projects={projects}
        selectedProjectId={contextProjectId}
        conversations={bindableConversations}
        selectedConversationId={selectedConversationId}
        prConversationOptions={prConversationOptions}
        selectedPrConversationId={selectedPrConversationId}
        selectedTicketProvider={selectedTicketProvider}
        selectedTicketId={selectedTicketId}
        selectedTicketConversationId={selectedTicketConversationId}
        ticketingProviders={bindableTicketingProviders}
        ticketOptions={ticketOptions}
        isConversationsLoading={conversationsQuery.isLoading}
        isPrConversationsLoading={githubOverviewQuery.isLoading || githubOverviewQuery.isFetching}
        isTicketingProvidersLoading={ticketingProvidersQuery.isLoading || ticketingProvidersQuery.isFetching}
        isTicketsLoading={ticketsForBindingQuery.isLoading || ticketsForBindingQuery.isFetching}
        isBindPending={bindGranolaConversation.isPending}
        isTicketBindPending={bindTicketToGranolaConversation.isPending}
        bindError={bindError}
        ticketBindError={ticketBindError}
        onProjectChange={(nextProjectId) => {
          setContextProjectId(nextProjectId);
          setSelectedConversationId("");
          setSelectedPrConversationId("");
          setSelectedTicketProvider("");
          setSelectedTicketId("");
          setSelectedTicketConversationId("");
        }}
        onConversationChange={setSelectedConversationId}
        onPrConversationChange={setSelectedPrConversationId}
        onTicketProviderChange={handleTicketProviderChange}
        onTicketChange={setSelectedTicketId}
        onTicketConversationChange={setSelectedTicketConversationId}
        onStartNew={handleStartContext}
        onBindExisting={() => bindGranolaConversation.mutate(selectedConversationId)}
        onBindExistingPr={() => bindGranolaConversation.mutate(selectedPrConversationId)}
        onBindExistingTicket={handleBindExistingTicket}
        onClose={() => setContextDialogOpen(false)}
      />
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
    </div>
  );
}
