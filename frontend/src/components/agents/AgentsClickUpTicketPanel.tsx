import { ExternalLink, Loader2, RefreshCw, Ticket } from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { openExternalTicketUrl } from "@/components/ticketing/ticketing-open-external";
import { useAfterPaint } from "@/components/ticketing/useAfterPaint";
import { useConversationTicket, useTicketDetail } from "@/hooks/useTicketing";

interface AgentsClickUpTicketPanelProps {
  conversationId: string | null;
  projectId: string | null;
}

export function AgentsClickUpTicketPanel({
  conversationId,
  projectId,
}: AgentsClickUpTicketPanelProps) {
  const readyForDetail = useAfterPaint(Boolean(conversationId));
  const conversationTicketQuery = useConversationTicket(conversationId);
  const linkedTicket = conversationTicketQuery.data;
  const isClickUpTicket = linkedTicket?.ticketRef.provider === "clickup";
  const detailInput = isClickUpTicket
    ? { provider: "clickup" as const, ticketRef: linkedTicket.ticketRef }
    : null;
  const detailQuery = useTicketDetail(detailInput, {
    enabled: readyForDetail && isClickUpTicket,
  });
  const ticket = detailQuery.data;
  const ticketLabel =
    ticket?.ref.key ?? linkedTicket?.ticketRef.key ?? ticket?.ref.id ?? linkedTicket?.ticketRef.id;
  const ticketUrl = ticket?.url ?? linkedTicket?.url;
  const assignees = ticket?.assignees ?? (ticket?.assignee ? [ticket.assignee] : []);

  return (
    <div
      className="flex h-full min-h-0 flex-col"
      data-project-id={projectId ?? undefined}
      style={{
        backgroundColor: "var(--bg-base)",
        color: "var(--text-primary)",
      }}
    >
      <div
        className="flex items-center justify-between gap-3 px-4 py-3"
        style={{
          borderBottomColor: "var(--border-subtle)",
          borderBottomStyle: "solid",
          borderBottomWidth: 1,
        }}
      >
        <div className="flex min-w-0 items-center gap-2">
          <Ticket className="h-4 w-4 shrink-0" style={{ color: "var(--accent-primary)" }} />
          <div className="min-w-0">
            <h2 className="text-sm font-semibold">ClickUp</h2>
            <p className="truncate text-xs" style={{ color: "var(--text-muted)" }}>
              {ticketLabel ?? "No task linked"}
            </p>
          </div>
        </div>
        <div className="flex items-center gap-1">
          {ticketUrl ? (
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  aria-label="Open ClickUp task"
                  onClick={() => void openExternalTicketUrl(ticketUrl)}
                >
                  <ExternalLink className="h-4 w-4" />
                </Button>
              </TooltipTrigger>
              <TooltipContent>Open ClickUp task</TooltipContent>
            </Tooltip>
          ) : null}
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                type="button"
                variant="ghost"
                size="icon"
                aria-label="Refresh ClickUp task"
                disabled={!isClickUpTicket || detailQuery.isFetching}
                onClick={() => void detailQuery.refetch()}
              >
                <RefreshCw className={detailQuery.isFetching ? "h-4 w-4 animate-spin" : "h-4 w-4"} />
              </Button>
            </TooltipTrigger>
            <TooltipContent>Refresh ClickUp task</TooltipContent>
          </Tooltip>
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto p-4">
        {conversationTicketQuery.isLoading || (isClickUpTicket && !readyForDetail) ? (
          <PanelStatus label="Loading ClickUp task" busy />
        ) : conversationTicketQuery.isError ? (
          <PanelStatus label="ClickUp link failed to load" />
        ) : !isClickUpTicket ? (
          <PanelStatus label="No ClickUp task linked to this conversation" />
        ) : detailQuery.isLoading ? (
          <PanelStatus label="Loading ClickUp task details" busy />
        ) : detailQuery.isError ? (
          <PanelStatus label="ClickUp task details could not be loaded" />
        ) : ticket ? (
          <div className="space-y-5">
            <div>
              <div className="flex flex-wrap items-center gap-2 text-xs">
                <span>{ticketLabel}</span>
                <span
                  className="rounded-full px-2 py-0.5"
                  style={{
                    backgroundColor: "var(--overlay-faint)",
                    color: "var(--text-secondary)",
                  }}
                >
                  {ticket.state.name}
                </span>
              </div>
              <h3 className="mt-2 text-base font-semibold">{ticket.title}</h3>
            </div>

            {assignees.length > 0 ? (
              <DetailGroup label="Assignees" values={assignees.map((assignee) => assignee.name)} />
            ) : null}
            {ticket.labels.length > 0 ? <DetailGroup label="Tags" values={ticket.labels} /> : null}

            <section>
              <h4 className="mb-2 text-xs font-semibold uppercase tracking-wide" style={{ color: "var(--text-muted)" }}>
                Description
              </h4>
              {ticket.descriptionMarkdown || ticket.descriptionText ? (
                <div className="prose prose-sm max-w-none break-words" style={{ color: "var(--text-primary)" }}>
                  <ReactMarkdown remarkPlugins={[remarkGfm]}>
                    {ticket.descriptionMarkdown ?? ticket.descriptionText ?? ""}
                  </ReactMarkdown>
                </div>
              ) : (
                <p className="text-sm" style={{ color: "var(--text-muted)" }}>
                  No description provided.
                </p>
              )}
            </section>
          </div>
        ) : null}
      </div>
    </div>
  );
}

function DetailGroup({ label, values }: { label: string; values: string[] }) {
  return (
    <section>
      <h4 className="mb-2 text-xs font-semibold uppercase tracking-wide" style={{ color: "var(--text-muted)" }}>
        {label}
      </h4>
      <div className="flex flex-wrap gap-2">
        {values.map((value) => (
          <span
            key={value}
            className="rounded-md px-2 py-1 text-xs"
            style={{ backgroundColor: "var(--bg-surface)", color: "var(--text-secondary)" }}
          >
            {value}
          </span>
        ))}
      </div>
    </section>
  );
}

function PanelStatus({ label, busy = false }: { label: string; busy?: boolean }) {
  return (
    <div className="flex min-h-32 items-center justify-center gap-2 text-sm" style={{ color: "var(--text-muted)" }}>
      {busy ? <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" /> : null}
      <span>{label}</span>
    </div>
  );
}
