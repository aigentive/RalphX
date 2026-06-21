import { useEffect, useMemo, useRef, useState } from "react";
import { ExternalLink, MessageSquare, Send, UserCheck, UserX, X } from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

import type {
  TicketAssociationItem,
  TicketAssociations,
  TicketComment,
  TicketDeepLink,
  TicketDetail,
  TicketingCapabilities,
  TicketSummary,
  TicketTransitionOption,
} from "@/api/ticketing";
import { markdownComponents } from "@/components/Chat/MessageItem.markdown";
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
import { TicketAssigneeChip } from "./TicketAssigneeChip";
import { TicketLabels } from "./TicketLabels";
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
  onClearAssignee?: (() => Promise<void> | void) | undefined;
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

function TicketMarkdown({ content }: { content: string }) {
  return (
    <div className="prose prose-sm prose-invert max-w-none text-sm leading-6 text-[var(--text-secondary)] prose-code:before:content-none prose-code:after:content-none">
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={markdownComponents}>
        {content}
      </ReactMarkdown>
    </div>
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
  onClearAssignee,
  onAddComment,
  isStartWorkPending,
  startWorkError,
  onNavigate,
  onStartWork,
  onClose,
}: TicketDetailSheetProps) {
  const [commentDraft, setCommentDraft] = useState("");
  const [localComments, setLocalComments] = useState<TicketComment[]>([]);
  const commentsSectionRef = useRef<HTMLElement | null>(null);
  const ticketIdentity = ticket ? `${ticket.ref.provider}:${ticket.ref.id}` : null;
  const providerComments = useMemo(
    () => ticket && "comments" in ticket ? ticket.comments : [],
    [ticket],
  );
  const visibleComments = useMemo(() => {
    if (localComments.length === 0) {
      return providerComments;
    }
    const providerCommentIds = new Set(
      providerComments.map((comment) => comment.id).filter(Boolean),
    );
    const providerCommentBodies = new Set(
      providerComments
        .map((comment) => (comment.bodyMarkdown || comment.bodyText).trim())
        .filter(Boolean),
    );
    return [
      ...providerComments,
      ...localComments.filter((comment) => {
        if (comment.id && providerCommentIds.has(comment.id)) {
          return false;
        }
        return !providerCommentBodies.has((comment.bodyMarkdown || comment.bodyText).trim());
      }),
    ];
  }, [localComments, providerComments]);

  useEffect(() => {
    setLocalComments([]);
  }, [ticketIdentity]);

  useEffect(() => {
    if (providerComments.length === 0 || localComments.length === 0) {
      return;
    }
    const providerCommentBodies = new Set(
      providerComments
        .map((comment) => (comment.bodyMarkdown || comment.bodyText).trim())
        .filter(Boolean),
    );
    setLocalComments((current) =>
      current.filter((comment) => !providerCommentBodies.has((comment.bodyMarkdown || comment.bodyText).trim())),
    );
  }, [providerComments, localComments.length]);
  const writableTransitions = transitions.filter((transition) => !transition.disabledReason);
  const statusDisabledReason = !capabilities?.statusWrite
    ? "Status write-back is not available for this provider."
    : writableTransitions.length === 0
      ? "No provider workflow transitions are available for this ticket."
      : null;
  const assignDisabledReason = !capabilities?.assignmentWrite
    ? "Assign-to-me is not available for this provider."
    : null;
  const canClearAssignee = !assignDisabledReason && Boolean(ticket?.assignee);
  const commentDisabledReason = !capabilities?.commentWrite
    ? "Comment write-back is not available for this provider."
    : null;
  const canAddComment = !commentDisabledReason && commentDraft.trim().length > 0 && !isCommentPending;
  const descriptionMarkdown = ticket && "descriptionMarkdown" in ticket && ticket.descriptionMarkdown
    ? ticket.descriptionMarkdown
    : null;

  function handleStatusChange(nextStateId: string) {
    const transition = transitions.find((item) => item.toStateId === nextStateId);
    if (!transition || transition.disabledReason || statusDisabledReason) {
      return;
    }
    void onTransitionTicket?.(transition);
  }

  function handleJumpToComments() {
    commentsSectionRef.current?.scrollIntoView({ behavior: "smooth", block: "start" });
  }

  async function handleAddComment() {
    if (!canAddComment) {
      return;
    }
    const bodyMarkdown = commentDraft.trim();
    const createdAt = new Date().toISOString();
    const optimisticComment: TicketComment = {
      id: `local:${createdAt}`,
      author: { name: "You" },
      bodyMarkdown,
      bodyText: bodyMarkdown,
      createdAt,
      updatedAt: createdAt,
    };
    setLocalComments((current) => [...current, optimisticComment]);
    try {
      await onAddComment?.(bodyMarkdown);
      setCommentDraft("");
    } catch {
      setLocalComments((current) => current.filter((comment) => comment.id !== optimisticComment.id));
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
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    className="h-7 px-2 text-xs"
                    onClick={handleJumpToComments}
                  >
                    <MessageSquare className="h-3.5 w-3.5" aria-hidden="true" />
                    Comments ({visibleComments.length})
                  </Button>
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
                {(ticket.project || ticket.labels.length > 0) && (
                  <div className="mt-3 flex flex-wrap items-center gap-2 text-xs text-[var(--text-muted)]">
                    {ticket.project && (
                      <span
                        className="rounded border px-2 py-1"
                        style={{ borderColor: "var(--border-subtle)" }}
                      >
                        {ticket.project}
                      </span>
                    )}
                    <TicketLabels labels={ticket.labels} max={ticket.labels.length} size="md" />
                  </div>
                )}

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
                  {ticket.assignee ? (
                    <span
                      className="inline-flex items-center gap-2 rounded-md px-2 py-1"
                      style={{
                        backgroundColor: "var(--bg-surface)",
                        borderColor: "var(--border-subtle)",
                        borderStyle: "solid",
                        borderWidth: "1px",
                      }}
                    >
                      <span className="text-[10px] font-semibold uppercase text-[var(--text-muted)]">
                        Assignee
                      </span>
                      <TicketAssigneeChip person={ticket.assignee} size="md" />
                    </span>
                  ) : (
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
                  )}
                  {canClearAssignee && (
                    <ControlTooltip reason={assignDisabledReason}>
                      <Button
                        type="button"
                        variant="outline"
                        size="sm"
                        disabled={Boolean(assignDisabledReason) || isAssignPending}
                        onClick={() => void onClearAssignee?.()}
                      >
                        <UserX className="h-3.5 w-3.5" aria-hidden="true" />
                        {isAssignPending ? "Clearing" : "Clear assignee"}
                      </Button>
                    </ControlTooltip>
                  )}
                </div>

                <section className="mt-5">
                  <h3 className="text-xs font-semibold uppercase text-[var(--text-muted)]">Description</h3>
                  <div className="mt-2">
                    {descriptionMarkdown ? (
                      <TicketMarkdown content={descriptionMarkdown} />
                    ) : (
                      <p className="text-sm leading-6 text-[var(--text-secondary)]">
                        {isDetailLoading ? "Loading ticket detail" : "No description provided."}
                      </p>
                    )}
                  </div>
                </section>

                <section ref={commentsSectionRef} className="mt-6">
                  <h3 className="text-xs font-semibold uppercase text-[var(--text-muted)]">
                    Comments ({visibleComments.length})
                  </h3>
                  {visibleComments.length > 0 ? (
                    <div className="mt-2 space-y-2">
                      {visibleComments.map((comment, index) => (
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
                          <div className="mt-2">
                            <TicketMarkdown content={comment.bodyMarkdown || comment.bodyText} />
                          </div>
                        </article>
                      ))}
                    </div>
                  ) : (
                    <p className="mt-2 text-sm leading-6 text-[var(--text-secondary)]">
                      No comments yet.
                    </p>
                  )}
                </section>

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
