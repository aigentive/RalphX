import { useState } from "react";
import { ExternalLink, Send, UserCheck, X } from "lucide-react";

import type {
  TicketAssociationItem,
  TicketAssociations,
  TicketDeepLink,
  TicketDetail,
  TicketingCapabilities,
  TicketSummary,
  TicketTransitionOption,
} from "@/api/ticketing";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Textarea } from "@/components/ui/textarea";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { categoryToken, formatTicketDate, providerLabel, ticketKey } from "./ticketing-utils";

interface TicketDetailSheetProps {
  open: boolean;
  ticket: TicketDetail | TicketSummary | null;
  capabilities: TicketingCapabilities | null;
  transitions: TicketTransitionOption[];
  associations: TicketAssociations | undefined;
  isDetailLoading: boolean;
  isAssociationsLoading: boolean;
  isTransitionPending: boolean;
  isAssignPending: boolean;
  isCommentPending: boolean;
  onTransitionTicket?: ((transition: TicketTransitionOption) => Promise<void> | void) | undefined;
  onAssignToMe?: (() => Promise<void> | void) | undefined;
  onAddComment?: ((bodyMarkdown: string) => Promise<void> | void) | undefined;
  isStartWorkPending?: boolean | undefined;
  startWorkError?: string | null | undefined;
  onNavigate?: ((deepLink: TicketDeepLink) => void) | undefined;
  onStartWork?: (() => void) | undefined;
  onClose: () => void;
}

const ASSOCIATION_GROUPS: Array<{
  key: keyof Omit<TicketAssociations, "fetchedAt">;
  label: string;
}> = [
  { key: "tasks", label: "Tasks" },
  { key: "conversations", label: "Conversations" },
  { key: "sessions", label: "Sessions" },
  { key: "proposals", label: "Proposals" },
  { key: "pullRequests", label: "Pull Requests" },
  { key: "specs", label: "Specs" },
  { key: "checks", label: "Checks" },
  { key: "qa", label: "QA" },
];

function AssociationCard({
  item,
  onNavigate,
}: {
  item: TicketAssociationItem;
  onNavigate?: ((deepLink: TicketDeepLink) => void) | undefined;
}) {
  return (
    <button
      type="button"
      className="w-full rounded-md px-3 py-2 text-left hover:bg-[var(--bg-hover)] focus-visible:outline-none focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]"
      style={{
        backgroundColor: "var(--bg-elevated)",
        borderColor: "var(--border-subtle)",
        borderStyle: "solid",
        borderWidth: "1px",
        color: "var(--text-primary)",
      }}
      onClick={() => onNavigate?.(item.deepLink)}
    >
      <div className="flex items-center justify-between gap-2">
        <span className="truncate text-sm font-medium">{item.title}</span>
        {item.active && (
          <span
            className="shrink-0 rounded-full px-2 py-0.5 text-[11px] font-medium text-[var(--text-primary)]"
            style={{
              backgroundColor: "var(--accent-muted)",
              borderColor: "var(--accent-border)",
              borderStyle: "solid",
              borderWidth: "1px",
            }}
          >
            Active
          </span>
        )}
      </div>
      {(item.status || item.subtitle) && (
        <p className="mt-1 truncate text-xs text-[var(--text-muted)]">
          {item.status ?? item.subtitle}
        </p>
      )}
    </button>
  );
}

function RalphxAssociationPanel({
  associations,
  isLoading,
  isStartWorkPending = false,
  startWorkError,
  onNavigate,
  onStartWork,
}: {
  associations: TicketAssociations | undefined;
  isLoading: boolean;
  isStartWorkPending?: boolean | undefined;
  startWorkError?: string | null | undefined;
  onNavigate?: ((deepLink: TicketDeepLink) => void) | undefined;
  onStartWork?: (() => void) | undefined;
}) {
  const activeCount = ASSOCIATION_GROUPS.reduce((count, group) => {
    return count + (associations?.[group.key].filter((item) => item.active).length ?? 0);
  }, 0);
  const totalCount = ASSOCIATION_GROUPS.reduce((count, group) => {
    return count + (associations?.[group.key].length ?? 0);
  }, 0);

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
        <h3 className="text-sm font-semibold text-[var(--text-primary)]">RalphX Work</h3>
        {activeCount > 0 && (
          <span
            className="rounded-full px-2 py-0.5 text-xs font-medium text-[var(--text-primary)]"
            style={{
              backgroundColor: "var(--accent-muted)",
              borderColor: "var(--accent-border)",
              borderStyle: "solid",
              borderWidth: "1px",
            }}
          >
            ● Active
          </span>
        )}
      </div>
      <Button
        type="button"
        variant="outline"
        size="sm"
        className="mt-3 w-full justify-center"
        disabled={!onStartWork || isStartWorkPending}
        onClick={onStartWork}
      >
        {isStartWorkPending ? "Starting..." : "Start RalphX work"}
      </Button>
      {startWorkError && (
        <p className="mt-2 text-xs text-[var(--status-error)]" role="alert">
          {startWorkError}
        </p>
      )}
      {isLoading ? (
        <p className="mt-4 text-sm text-[var(--text-muted)]">Loading associations</p>
      ) : totalCount === 0 ? (
        <p className="mt-4 text-sm text-[var(--text-muted)]">No RalphX links yet.</p>
      ) : (
        <div className="mt-4 min-h-0 space-y-4 overflow-auto">
          {ASSOCIATION_GROUPS.map((group) => {
            const items = associations?.[group.key] ?? [];
            if (items.length === 0) {
              return null;
            }
            return (
              <section key={group.key}>
                <h4 className="mb-2 text-[11px] font-semibold uppercase text-[var(--text-muted)]">
                  {group.label} ({items.length})
                </h4>
                <div className="space-y-2">
                  {items.map((item) => (
                    <AssociationCard
                      key={`${group.key}:${item.id}`}
                      item={item}
                      onNavigate={onNavigate}
                    />
                  ))}
                </div>
              </section>
            );
          })}
        </div>
      )}
    </aside>
  );
}

function ControlTooltip({
  reason,
  children,
}: {
  reason: string | null;
  children: React.ReactNode;
}) {
  if (!reason) {
    return <>{children}</>;
  }
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span className="inline-flex">{children}</span>
      </TooltipTrigger>
      <TooltipContent>{reason}</TooltipContent>
    </Tooltip>
  );
}

export function TicketDetailSheet({
  open,
  ticket,
  capabilities,
  transitions,
  associations,
  isDetailLoading,
  isAssociationsLoading,
  isTransitionPending,
  isAssignPending,
  isCommentPending,
  onTransitionTicket,
  onAssignToMe,
  onAddComment,
  isStartWorkPending,
  startWorkError,
  onNavigate,
  onStartWork,
  onClose,
}: TicketDetailSheetProps) {
  const [commentDraft, setCommentDraft] = useState("");
  const writableTransitions = transitions.filter((transition) => !transition.disabledReason);
  const statusDisabledReason = !capabilities?.statusWrite
    ? "Status write-back is not available for this provider."
    : writableTransitions.length === 0
      ? "No provider workflow transitions are available for this ticket."
      : null;
  const assignDisabledReason = !capabilities?.assignmentWrite
    ? "Assign-to-me is not available for this provider."
    : null;
  const commentDisabledReason = !capabilities?.commentWrite
    ? "Comment write-back is not available for this provider."
    : null;
  const canAddComment = !commentDisabledReason && commentDraft.trim().length > 0 && !isCommentPending;

  function handleStatusChange(nextStateId: string) {
    const transition = transitions.find((item) => item.toStateId === nextStateId);
    if (!transition || transition.disabledReason || statusDisabledReason) {
      return;
    }
    void onTransitionTicket?.(transition);
  }

  async function handleAddComment() {
    if (!canAddComment) {
      return;
    }
    const bodyMarkdown = commentDraft.trim();
    try {
      await onAddComment?.(bodyMarkdown);
      setCommentDraft("");
    } catch {
      // Rollback and visible errors are owned by the mutation hook; preserve the draft.
    }
  }

  return (
    <Dialog open={open} onOpenChange={(nextOpen) => {
      if (!nextOpen) {
        onClose();
      }
    }}>
      <DialogContent
        hideCloseButton
        className="left-auto right-0 top-12 h-[calc(100vh-3rem)] max-w-[880px] translate-x-0 translate-y-0 rounded-none p-0"
        style={{
          backgroundColor: "var(--bg-elevated)",
          borderColor: "var(--border-subtle)",
          borderStyle: "solid",
          borderWidth: "1px",
          boxShadow: "var(--shadow-lg)",
        }}
      >
        {ticket ? (
          <div className="grid h-full min-h-0 grid-cols-[minmax(0,1fr)_320px]">
            <div className="flex min-h-0 flex-col">
              <DialogHeader className="shrink-0 px-5 py-4">
                <div className="min-w-0">
                  <DialogTitle className="truncate text-base">
                    {ticketKey(ticket.ref)} · {providerLabel(ticket.ref.provider)}
                  </DialogTitle>
                  <DialogDescription className="mt-1 truncate">
                    {ticket.title}
                  </DialogDescription>
                </div>
                <Button type="button" variant="ghost" size="sm" onClick={onClose}>
                  <X className="h-4 w-4" aria-hidden="true" />
                  Close
                </Button>
              </DialogHeader>
              <div className="min-h-0 flex-1 overflow-auto p-5">
                <div className="flex flex-wrap items-center gap-2 text-sm">
                  <span className="inline-flex items-center gap-1.5 rounded-full px-2 py-1 text-xs font-medium">
                    <span
                      className="h-2 w-2 rounded-full"
                      aria-hidden="true"
                      style={{ backgroundColor: categoryToken(ticket.state.category) }}
                    />
                    {ticket.state.name}
                  </span>
                  <span className="text-xs text-[var(--text-muted)]">
                    Updated {formatTicketDate(ticket.updatedAt)}
                  </span>
                  {ticket.url && (
                    <a
                      href={ticket.url}
                      target="_blank"
                      rel="noreferrer"
                      className="inline-flex items-center gap-1 text-xs font-medium text-[var(--status-info)]"
                    >
                      Open in provider
                      <ExternalLink className="h-3.5 w-3.5" aria-hidden="true" />
                    </a>
                  )}
                </div>

                <div className="mt-4 flex flex-wrap items-center gap-2">
                  <ControlTooltip reason={statusDisabledReason}>
                    <select
                      aria-label="Ticket status"
                      value={ticket.state.id}
                      disabled={Boolean(statusDisabledReason) || isTransitionPending}
                      onChange={(event) => handleStatusChange(event.target.value)}
                      className="h-8 min-w-[160px] rounded-md px-2 text-xs outline-none focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px] disabled:cursor-not-allowed disabled:opacity-50"
                      style={{
                        backgroundColor: "var(--bg-surface)",
                        borderColor: "var(--border-subtle)",
                        borderStyle: "solid",
                        borderWidth: "1px",
                        color: "var(--text-primary)",
                      }}
                    >
                      <option value={ticket.state.id}>{ticket.state.name}</option>
                      {transitions
                        .filter((transition) => transition.toStateId !== ticket.state.id)
                        .map((transition) => (
                          <option
                            key={`${transition.toStateId}:${transition.providerTransitionId ?? ""}`}
                            value={transition.toStateId}
                            disabled={Boolean(transition.disabledReason)}
                          >
                            {transition.name}
                          </option>
                        ))}
                    </select>
                  </ControlTooltip>
                  <ControlTooltip reason={assignDisabledReason}>
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      disabled={Boolean(assignDisabledReason) || isAssignPending}
                      onClick={() => void onAssignToMe?.()}
                    >
                      <UserCheck className="h-3.5 w-3.5" aria-hidden="true" />
                      {isAssignPending ? "Assigning" : "Assign to me"}
                    </Button>
                  </ControlTooltip>
                </div>

                <section className="mt-5">
                  <h3 className="text-xs font-semibold uppercase text-[var(--text-muted)]">Description</h3>
                  <p className="mt-2 whitespace-pre-wrap text-sm leading-6 text-[var(--text-secondary)]">
                    {"descriptionMarkdown" in ticket && ticket.descriptionMarkdown
                      ? ticket.descriptionMarkdown
                      : isDetailLoading
                        ? "Loading ticket detail"
                        : "No description provided."}
                  </p>
                </section>

                {"comments" in ticket && ticket.comments.length > 0 && (
                  <section className="mt-6">
                    <h3 className="text-xs font-semibold uppercase text-[var(--text-muted)]">
                      Comments ({ticket.comments.length})
                    </h3>
                    <div className="mt-2 space-y-2">
                      {ticket.comments.map((comment, index) => (
                        <article
                          key={comment.id ?? `comment-${index}`}
                          className="rounded-md p-3"
                          style={{
                            backgroundColor: "var(--bg-surface)",
                            borderColor: "var(--border-subtle)",
                            borderStyle: "solid",
                            borderWidth: "1px",
                          }}
                        >
                          <p className="text-xs font-medium text-[var(--text-secondary)]">
                            {comment.author?.name ?? "Provider comment"}
                          </p>
                          <p className="mt-1 text-sm text-[var(--text-primary)]">
                            {comment.bodyText || comment.bodyMarkdown}
                          </p>
                        </article>
                      ))}
                    </div>
                  </section>
                )}

                <section className="mt-6">
                  <h3 className="text-xs font-semibold uppercase text-[var(--text-muted)]">Add comment</h3>
                  <div className="mt-2 space-y-2">
                    <Textarea
                      aria-label="Ticket comment"
                      value={commentDraft}
                      disabled={Boolean(commentDisabledReason) || isCommentPending}
                      onChange={(event) => setCommentDraft(event.target.value)}
                      placeholder="Write a provider comment"
                      className="min-h-20 text-sm"
                      style={{
                        backgroundColor: "var(--bg-surface)",
                        borderColor: "var(--border-subtle)",
                        borderStyle: "solid",
                        borderWidth: "1px",
                        color: "var(--text-primary)",
                      }}
                    />
                    <div className="flex justify-end">
                      <ControlTooltip reason={commentDisabledReason}>
                        <Button
                          type="button"
                          size="sm"
                          disabled={!canAddComment}
                          onClick={() => void handleAddComment()}
                        >
                          <Send className="h-3.5 w-3.5" aria-hidden="true" />
                          {isCommentPending ? "Posting" : "Add comment"}
                        </Button>
                      </ControlTooltip>
                    </div>
                  </div>
                </section>
              </div>
            </div>
            <RalphxAssociationPanel
              associations={associations}
              isLoading={isAssociationsLoading}
              isStartWorkPending={isStartWorkPending}
              startWorkError={startWorkError}
              onNavigate={onNavigate}
              onStartWork={onStartWork}
            />
          </div>
        ) : (
          <div className="p-6 text-sm text-[var(--text-muted)]">Loading ticket</div>
        )}
      </DialogContent>
    </Dialog>
  );
}
