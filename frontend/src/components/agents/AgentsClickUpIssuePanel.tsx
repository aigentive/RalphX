import { ExternalLink, Loader2, RefreshCw, Ticket } from "lucide-react";
import { useMemo, type ReactNode } from "react";

import { TicketDetailReadOnlyContent } from "@/components/ticketing/TicketDetailReadOnlyContent";
import { Button } from "@/components/ui/button";
import { useConversationTicket, useTicketDetail } from "@/hooks/useTicketing";

import { ArtifactSelectionSource } from "./artifact-selection/ArtifactSelectionSource";
import { buildTicketSelectionContent } from "./artifact-selection/ticketSelectionContent";

interface AgentsClickUpIssuePanelProps {
  conversationId: string | null;
}

export function AgentsClickUpIssuePanel({
  conversationId,
}: AgentsClickUpIssuePanelProps) {
  const conversationTicketQuery = useConversationTicket(conversationId);
  const binding =
    conversationTicketQuery.data?.ticketRef.provider === "clickup"
      ? conversationTicketQuery.data
      : null;
  const detailInput = binding
    ? { provider: "clickup" as const, ticketRef: binding.ticketRef }
    : null;
  const detailQuery = useTicketDetail(detailInput, {
    enabled: Boolean(detailInput),
  });
  const ticket = detailQuery.data ?? null;
  const selectionContent = useMemo(
    () =>
      ticket
        ? buildTicketSelectionContent({
            key: ticket.ref.key ?? ticket.ref.id,
            title: ticket.title,
            status: ticket.state.name,
            assignees: ticket.assignees?.map((assignee) => assignee.name),
            reporter: ticket.reporter?.name,
            description:
              ticket.descriptionMarkdown ?? ticket.descriptionText ?? null,
            acceptanceCriteria: ticket.acceptanceCriteriaMarkdown,
            comments: ticket.comments.map((comment) => ({
              author: comment.author?.name,
              body: comment.bodyMarkdown || comment.bodyText,
            })),
          })
        : null,
    [ticket],
  );

  const displayKey =
    ticket?.ref.key ?? binding?.ticketRef.key ?? ticket?.ref.id ?? binding?.ticketRef.id;
  const title = ticket?.title ?? binding?.title ?? null;
  const url = ticket?.url ?? binding?.url ?? null;

  return (
    <div
      className="flex h-full min-h-0 flex-col"
      style={{
        backgroundColor: "var(--bg-base)",
        color: "var(--text-primary)",
      }}
    >
      <div
        className="flex min-h-14 items-center gap-3 border-b px-4 py-3"
        style={{
          backgroundColor: "var(--bg-surface)",
          borderBottomColor: "var(--border-subtle)",
          borderBottomStyle: "solid",
          borderBottomWidth: 1,
        }}
      >
        <Ticket
          className="h-4 w-4 shrink-0"
          style={{ color: "var(--accent-primary)" }}
        />
        <div className="min-w-0 flex-1">
          <h2 className="truncate text-sm font-semibold">
            {displayKey ?? "ClickUp"}
          </h2>
          <p className="truncate text-xs" style={{ color: "var(--text-muted)" }}>
            {title ?? "No ClickUp task assigned"}
          </p>
        </div>
        {url ? (
          <a
            href={url}
            target="_blank"
            rel="noreferrer"
            aria-label="Open ClickUp task"
            className="inline-flex items-center gap-1 rounded-md px-2 py-1 text-xs font-medium"
            style={{ color: "var(--accent-primary)" }}
          >
            <ExternalLink className="h-3.5 w-3.5" />
            Open
          </a>
        ) : null}
      </div>

      {ticket && selectionContent ? (
        <ArtifactSelectionSource
          conversationId={conversationId}
          source={{
            sourceType: "ticket",
            sourceKind: "clickup",
            sourceId: ticket.ref.id,
            ...(ticket.ref.key ? { sourceKey: ticket.ref.key } : {}),
            sourceTitle: ticket.title,
            provider: "clickup",
            sourceRevision: ticket.updatedAt,
          }}
          content={selectionContent}
        />
      ) : null}

      <div className="min-h-0 flex-1 overflow-y-auto p-4">
        {!conversationId ? (
          <PanelStatus label="No conversation selected" />
        ) : conversationTicketQuery.isLoading || (binding && detailQuery.isLoading) ? (
          <PanelStatus label="Loading ClickUp task" busy />
        ) : !binding ? (
          <PanelStatus label="No ClickUp task assigned" />
        ) : detailQuery.error ? (
          <PanelStatus
            label="Could not load the ClickUp task"
            action={
              <Button
                type="button"
                variant="outline"
                size="sm"
                aria-label="Refresh ClickUp task"
                disabled={detailQuery.isFetching}
                onClick={() => void detailQuery.refetch()}
              >
                <RefreshCw
                  className={detailQuery.isFetching ? "animate-spin" : undefined}
                  aria-hidden="true"
                />
                Refresh
              </Button>
            }
          />
        ) : ticket ? (
          <div className="space-y-4">
            <div className="flex items-center justify-between gap-3">
              <p className="text-sm font-medium">{ticket.title}</p>
              <span
                className="shrink-0 rounded px-2 py-1 text-xs"
                style={{
                  backgroundColor: "var(--overlay-faint)",
                  color: "var(--text-secondary)",
                }}
              >
                {ticket.state.name}
              </span>
            </div>
            <TicketDetailReadOnlyContent ticket={ticket} />
          </div>
        ) : null}
      </div>
    </div>
  );
}

function PanelStatus({
  label,
  busy = false,
  action,
}: {
  label: string;
  busy?: boolean;
  action?: ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-3">
      <div
        className="flex items-center gap-2 text-sm"
        style={{ color: "var(--text-muted)" }}
      >
        {busy ? <Loader2 className="h-4 w-4 animate-spin" /> : null}
        <span>{label}</span>
      </div>
      {action}
    </div>
  );
}
