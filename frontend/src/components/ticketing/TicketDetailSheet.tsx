import { useEffect, useMemo, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  ExternalLink,
  FolderKanban,
  MessageSquare,
  Send,
  UserCheck,
  UserX,
  X,
} from "lucide-react";

import { granolaApi, type GranolaNoteSummary } from "@/api/granola";
import type {
  TicketAssociations,
  TicketComment,
  TicketDeepLink,
  TicketDetail,
  TicketingCapabilities,
  TicketLabelOption,
  TicketSummary,
  TicketTransitionOption,
} from "@/api/ticketing";
import { invalidateAgentConversationGranolaNote } from "@/components/agents/agentGranolaNoteQueries";
import { RalphxAssociationPanel } from "@/components/associations/RalphxAssociationPanel";
import { granolaDashboardKeys } from "@/components/granola/granolaDashboardKeys";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Textarea } from "@/components/ui/textarea";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { TicketAssigneeChips } from "./TicketAssigneeChip";
import { TicketLabelEditor } from "./TicketLabelEditor";
import { TicketLabels } from "./TicketLabels";
import { TicketSearchableSelect } from "./TicketSearchableSelect";
import { TicketDetailReadOnlyContent } from "./TicketDetailReadOnlyContent";
import { openExternalTicketUrl } from "./ticketing-open-external";
import {
  countNewComments,
  sortCommentsByCreatedAt,
  ticketAssignees,
} from "./ticketing-read-state";
import {
  categoryToken,
  formatTicketDate,
  providerLabel,
  ticketKey,
} from "./ticketing-utils";

interface TicketDetailSheetProps {
  open: boolean;
  ticket: TicketDetail | TicketSummary | null;
  capabilities: TicketingCapabilities | null;
  transitions: TicketTransitionOption[];
  associations: TicketAssociations | undefined;
  projectId?: string | undefined;
  isDetailLoading: boolean;
  isAssociationsLoading: boolean;
  isTransitionPending: boolean;
  isAssignPending: boolean;
  isCommentPending: boolean;
  isLabelPending?: boolean | undefined;
  /** Selectable team labels for Linear pick-list mode. */
  labelOptions?: TicketLabelOption[] | undefined;
  isLabelOptionsLoading?: boolean | undefined;
  onTransitionTicket?:
    ((transition: TicketTransitionOption) => Promise<void> | void) | undefined;
  onAssignToMe?: (() => Promise<void> | void) | undefined;
  onClearAssignee?: (() => Promise<void> | void) | undefined;
  onAddComment?: ((bodyMarkdown: string) => Promise<void> | void) | undefined;
  onSetLabels?: ((labels: string[]) => Promise<void> | void) | undefined;
  /** Timestamp the viewer last opened this ticket; comments newer than it are "new". */
  seenUntil?: string | null | undefined;
  isStartWorkPending?: boolean | undefined;
  startWorkError?: string | null | undefined;
  showStartWork?: boolean | undefined;
  /** Existing project conversations the viewer may bind to this ticket. */
  bindableConversations?: { id: string; title: string | null }[] | undefined;
  onBindConversation?: ((conversationId: string) => void) | undefined;
  isBindPending?: boolean | undefined;
  bindError?: string | null | undefined;
  onNavigate?: ((deepLink: TicketDeepLink) => void) | undefined;
  onStartWork?: (() => void) | undefined;
  /**
   * Whether the start-work + bind-conversation affordances are shown. Providers
   * without a manual conversation-binding mutation pass `false` so
   * the non-functional affordance stays hidden. Defaults to `true`.
   */
  showConversationBinding?: boolean | undefined;
  onClose: () => void;
}

const EMPTY_GRANOLA_NOTES: GranolaNoteSummary[] = [];

function normalizeAssociationKey(value: string | null | undefined): string {
  return (value ?? "").trim().toLowerCase();
}

function granolaNoteMatchesTicket(
  note: GranolaNoteSummary,
  ticket: TicketDetail | TicketSummary | null,
  conversationIds: Set<string>,
): boolean {
  if (
    conversationIds.size > 0 &&
    (note.rxConversations ?? []).some((conversation) =>
      conversationIds.has(normalizeAssociationKey(conversation.conversationId)),
    )
  ) {
    return true;
  }
  if (!ticket) {
    return false;
  }
  const provider = normalizeAssociationKey(ticket.ref.provider);
  const ticketKeys = [ticket.ref.key, ticket.ref.id]
    .map(normalizeAssociationKey)
    .filter(Boolean);
  return (note.ticketLinks ?? []).some((ticketLink) => {
    const linkProvider = normalizeAssociationKey(ticketLink.provider);
    const linkLabel = normalizeAssociationKey(ticketLink.label);
    return ticketKeys.some(
      (key) =>
        linkLabel === key &&
        (linkProvider === provider ||
          (provider === "jira" && linkProvider === "atlassian")),
    );
  });
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
  projectId,
  isDetailLoading,
  isAssociationsLoading,
  isTransitionPending,
  isAssignPending,
  isCommentPending,
  isLabelPending,
  labelOptions,
  isLabelOptionsLoading,
  onTransitionTicket,
  onAssignToMe,
  onClearAssignee,
  onAddComment,
  onSetLabels,
  seenUntil,
  isStartWorkPending,
  startWorkError,
  showStartWork = true,
  bindableConversations,
  onBindConversation,
  isBindPending,
  bindError,
  onNavigate,
  onStartWork,
  showConversationBinding = true,
  onClose,
}: TicketDetailSheetProps) {
  const queryClient = useQueryClient();
  const [commentDraft, setCommentDraft] = useState("");
  const [localComments, setLocalComments] = useState<TicketComment[]>([]);
  const commentsSectionRef = useRef<HTMLElement | null>(null);
  const ticketIdentity = ticket
    ? `${ticket.ref.provider}:${ticket.ref.id}`
    : null;
  const providerComments = useMemo(
    () => (ticket && "comments" in ticket ? ticket.comments : []),
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
        return !providerCommentBodies.has(
          (comment.bodyMarkdown || comment.bodyText).trim(),
        );
      }),
    ];
  }, [localComments, providerComments]);

  const sortedComments = useMemo(
    () => sortCommentsByCreatedAt(visibleComments),
    [visibleComments],
  );
  const seenUntilValue = seenUntil ?? null;
  const newCommentCount = countNewComments(sortedComments, seenUntilValue);

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
      current.filter(
        (comment) =>
          !providerCommentBodies.has(
            (comment.bodyMarkdown || comment.bodyText).trim(),
          ),
      ),
    );
  }, [providerComments, localComments.length]);
  const writableTransitions = transitions.filter(
    (transition) => !transition.disabledReason,
  );
  const statusDisabledReason = !capabilities?.statusWrite
    ? "Status write-back is not available for this provider."
    : writableTransitions.length === 0
      ? "No provider workflow transitions are available for this ticket."
      : null;
  const assignDisabledReason = !capabilities?.assignmentWrite
    ? "Assign-to-me is not available for this provider."
    : null;
  const assignees = ticket ? ticketAssignees(ticket) : [];
  const canClearAssignee = !assignDisabledReason && assignees.length > 0;
  const commentDisabledReason = !capabilities?.commentWrite
    ? "Comment write-back is not available for this provider."
    : null;
  const canAddComment =
    !commentDisabledReason &&
    commentDraft.trim().length > 0 &&
    !isCommentPending;
  const labelDisabledReason = !capabilities?.labelWrite
    ? "Label editing is not available for this provider."
    : null;
  const associationConversationIds = useMemo(
    () =>
      new Set(
        (associations?.conversations ?? [])
          .map((conversation) =>
            normalizeAssociationKey(conversation.deepLink.id),
          )
          .filter(Boolean),
      ),
    [associations?.conversations],
  );
  const effectiveProjectId =
    projectId ??
    associations?.conversations.find(
      (conversation) => conversation.deepLink.projectId,
    )?.deepLink.projectId ??
    null;
  const granolaSettingsQuery = useQuery({
    queryKey: granolaDashboardKeys.settings(),
    queryFn: () => granolaApi.getSettings(),
    enabled: open && Boolean(effectiveProjectId),
    staleTime: 30_000,
  });
  const granolaEnabled =
    granolaSettingsQuery.data?.enabled === true &&
    granolaSettingsQuery.data.hasApiToken === true &&
    granolaSettingsQuery.data.validationStatus === "valid";
  const granolaNotesQuery = useQuery({
    queryKey: effectiveProjectId
      ? granolaDashboardKeys.notes(effectiveProjectId)
      : [...granolaDashboardKeys.all, "notes", null],
    queryFn: () =>
      granolaApi.listNotes({
        pageSize: 100,
        ...(effectiveProjectId ? { projectId: effectiveProjectId } : {}),
      }),
    enabled: open && granolaEnabled && Boolean(effectiveProjectId),
    staleTime: 20_000,
  });
  const allGranolaNotes = granolaNotesQuery.data?.notes ?? EMPTY_GRANOLA_NOTES;
  const linkedGranolaNotes = useMemo(
    () =>
      allGranolaNotes.filter((note) =>
        granolaNoteMatchesTicket(note, ticket, associationConversationIds),
      ),
    [allGranolaNotes, associationConversationIds, ticket],
  );
  const linkedGranolaNoteIds = useMemo(
    () => new Set(linkedGranolaNotes.map((note) => note.id)),
    [linkedGranolaNotes],
  );
  const availableGranolaNotes = useMemo(
    () => allGranolaNotes.filter((note) => !linkedGranolaNoteIds.has(note.id)),
    [allGranolaNotes, linkedGranolaNoteIds],
  );
  const bindGranolaNote = useMutation({
    mutationFn: async ({
      noteId,
      conversationId,
    }: {
      noteId: string;
      conversationId: string;
    }) => {
      const note = allGranolaNotes.find((candidate) => candidate.id === noteId);
      if (!note || !conversationId) {
        throw new Error("Select a Granola note and RalphX conversation.");
      }
      return granolaApi.assignAgentConversationGranolaNote({
        conversationId,
        projectId: effectiveProjectId,
        noteId: note.id,
        title: note.title ?? null,
        noteUrl: note.url ?? null,
        summary: note.summary ?? null,
        includeTranscript: true,
        refresh: true,
      });
    },
    onSuccess: (_note, variables) => {
      void invalidateAgentConversationGranolaNote(
        queryClient,
        variables.conversationId,
      );
      if (effectiveProjectId) {
        void queryClient.invalidateQueries({
          queryKey: granolaDashboardKeys.notes(effectiveProjectId),
        });
      }
    },
  });
  const granolaBindError =
    bindGranolaNote.error instanceof Error
      ? bindGranolaNote.error.message
      : bindGranolaNote.error
        ? "Granola note could not be bound."
        : null;

  function handleStatusChange(nextStateId: string) {
    const transition = transitions.find(
      (item) => item.toStateId === nextStateId,
    );
    if (!transition || transition.disabledReason || statusDisabledReason) {
      return;
    }
    void onTransitionTicket?.(transition);
  }

  function handleJumpToComments() {
    commentsSectionRef.current?.scrollIntoView({
      behavior: "smooth",
      block: "start",
    });
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
      attachments: [],
    };
    setLocalComments((current) => [...current, optimisticComment]);
    try {
      await onAddComment?.(bodyMarkdown);
      setCommentDraft("");
    } catch {
      setLocalComments((current) =>
        current.filter((comment) => comment.id !== optimisticComment.id),
      );
      // Rollback and visible errors are owned by the mutation hook; preserve the draft.
    }
  }

  return (
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
        {ticket ? (
          <div className="grid h-full min-h-0 grid-cols-[minmax(0,1fr)_320px]">
            <div className="flex min-h-0 flex-col">
              <DialogHeader className="shrink-0 px-5 py-4">
                <div className="min-w-0">
                  <DialogTitle className="truncate text-base">
                    {ticketKey(ticket.ref)} ·{" "}
                    {providerLabel(ticket.ref.provider)}
                  </DialogTitle>
                  <DialogDescription className="mt-1 truncate">
                    {ticket.title}
                  </DialogDescription>
                </div>
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  onClick={onClose}
                >
                  <X className="h-4 w-4" aria-hidden="true" />
                  Close
                </Button>
              </DialogHeader>
              <div className="min-h-0 flex-1 overflow-auto p-5">
                <div className="flex flex-wrap items-center gap-x-3 gap-y-2 text-sm">
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
                    <MessageSquare
                      className="h-3.5 w-3.5"
                      aria-hidden="true"
                      style={
                        newCommentCount > 0
                          ? { color: "var(--accent-primary)" }
                          : undefined
                      }
                    />
                    Comments ({sortedComments.length})
                    {newCommentCount > 0 && (
                      <span
                        className="font-semibold"
                        style={{ color: "var(--accent-primary)" }}
                      >
                        · {newCommentCount} new
                      </span>
                    )}
                  </Button>
                  {ticket.url && (
                    <a
                      href={ticket.url}
                      target="_blank"
                      rel="noreferrer"
                      className="inline-flex items-center gap-1 text-xs font-medium text-[var(--status-info)] focus-visible:outline-none focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]"
                      onClick={(event) => {
                        // WKWebView does not reliably open target="_blank" links in
                        // the system browser; route through the app opener (keeping
                        // the anchor for semantics/accessibility).
                        event.preventDefault();
                        if (ticket.url) {
                          void openExternalTicketUrl(ticket.url);
                        }
                      }}
                    >
                      Open in provider
                      <ExternalLink
                        className="h-3.5 w-3.5"
                        aria-hidden="true"
                      />
                    </a>
                  )}
                </div>
                {(ticket.project ||
                  (!capabilities?.labelWrite && ticket.labels.length > 0)) && (
                  <div className="mt-3 flex flex-wrap items-center gap-2 text-xs text-[var(--text-muted)]">
                    {ticket.project && (
                      <span
                        className="inline-flex items-center gap-1 rounded px-2 py-1 font-medium text-[var(--text-secondary)]"
                        title={`Project: ${ticket.project}`}
                        style={{
                          backgroundColor: "var(--bg-elevated)",
                          borderColor: "var(--border-subtle)",
                          borderStyle: "solid",
                          borderWidth: "1px",
                        }}
                      >
                        <FolderKanban
                          className="h-3 w-3 shrink-0 text-[var(--text-muted)]"
                          aria-hidden="true"
                        />
                        {ticket.project}
                      </span>
                    )}
                    {!capabilities?.labelWrite && (
                      <TicketLabels
                        labels={ticket.labels}
                        max={ticket.labels.length}
                        size="md"
                      />
                    )}
                  </div>
                )}
                {capabilities?.labelWrite && (
                  <div className="mt-3">
                    <TicketLabelEditor
                      provider={ticket.ref.provider}
                      labels={ticket.labels}
                      labelOptions={labelOptions}
                      isLabelOptionsLoading={isLabelOptionsLoading}
                      isLabelPending={isLabelPending}
                      disabledReason={labelDisabledReason}
                      onSetLabels={onSetLabels}
                    />
                  </div>
                )}

                <div className="mt-5 grid grid-cols-[84px_minmax(0,1fr)] items-center gap-x-4 gap-y-3 text-sm">
                  <span className="text-[11px] font-semibold uppercase tracking-wide text-[var(--text-muted)]">
                    Status
                  </span>
                  <div className="flex min-w-0 items-center gap-2">
                    <ControlTooltip reason={statusDisabledReason}>
                      <TicketSearchableSelect
                        ariaLabel="Ticket status"
                        value={ticket.state.id}
                        disabled={
                          Boolean(statusDisabledReason) || isTransitionPending
                        }
                        onValueChange={handleStatusChange}
                        className="min-w-[180px] max-w-[280px] text-xs"
                        searchPlaceholder="Search statuses..."
                        options={[
                          {
                            value: ticket.state.id,
                            label: ticket.state.name,
                            leadingColor: categoryToken(ticket.state.category),
                          },
                          ...transitions
                            .filter(
                              (transition) =>
                                transition.toStateId !== ticket.state.id &&
                                transition.name.trim().toLowerCase() !==
                                  ticket.state.name.trim().toLowerCase(),
                            )
                            .map((transition) => ({
                              value: transition.toStateId,
                              label: transition.name,
                              disabled: Boolean(transition.disabledReason),
                              description:
                                transition.disabledReason ?? undefined,
                              leadingColor: categoryToken(transition.category),
                            })),
                        ]}
                      />
                    </ControlTooltip>
                  </div>

                  <span className="text-[11px] font-semibold uppercase tracking-wide text-[var(--text-muted)]">
                    Assignee
                  </span>
                  <div className="flex min-w-0 items-center gap-2">
                    {assignees.length > 0 ? (
                      <>
                        <TicketAssigneeChips people={assignees} size="md" />
                        {canClearAssignee && (
                          <ControlTooltip reason={assignDisabledReason}>
                            <Button
                              type="button"
                              variant="ghost"
                              size="sm"
                              className="h-7 gap-1 px-2 text-[var(--text-muted)]"
                              disabled={
                                Boolean(assignDisabledReason) || isAssignPending
                              }
                              onClick={() => void onClearAssignee?.()}
                              aria-label="Clear assignee"
                              title="Clear assignee"
                            >
                              <UserX
                                className="h-3.5 w-3.5"
                                aria-hidden="true"
                              />
                              {isAssignPending ? "Clearing" : "Clear"}
                            </Button>
                          </ControlTooltip>
                        )}
                      </>
                    ) : (
                      <ControlTooltip reason={assignDisabledReason}>
                        <Button
                          type="button"
                          variant="outline"
                          size="sm"
                          disabled={
                            Boolean(assignDisabledReason) || isAssignPending
                          }
                          onClick={() => void onAssignToMe?.()}
                        >
                          <UserCheck
                            className="h-3.5 w-3.5"
                            aria-hidden="true"
                          />
                          {isAssignPending ? "Assigning" : "Assign to me"}
                        </Button>
                      </ControlTooltip>
                    )}
                  </div>
                </div>

                <TicketDetailReadOnlyContent
                  ticket={ticket}
                  comments={visibleComments}
                  isDetailLoading={isDetailLoading}
                  seenUntil={seenUntilValue}
                  commentsSectionRef={commentsSectionRef}
                />

                <section className="mt-6">
                  <h3 className="text-xs font-semibold uppercase text-[var(--text-muted)]">
                    Add comment
                  </h3>
                  <div className="mt-2 space-y-2">
                    <Textarea
                      aria-label="Ticket comment"
                      value={commentDraft}
                      disabled={
                        Boolean(commentDisabledReason) || isCommentPending
                      }
                      onChange={(event) => setCommentDraft(event.target.value)}
                      onKeyDown={(event) => {
                        // Cmd/Ctrl+Enter submits, matching the convention in
                        // Linear/GitHub/Slack comment composers.
                        if (
                          event.key === "Enter" &&
                          (event.metaKey || event.ctrlKey) &&
                          canAddComment
                        ) {
                          event.preventDefault();
                          void handleAddComment();
                        }
                      }}
                      placeholder="Write a provider comment (⌘/Ctrl+Enter to send)"
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
              ticket={ticket}
              associations={associations}
              isLoading={isAssociationsLoading}
              isStartWorkPending={isStartWorkPending}
              startWorkError={startWorkError}
              showStartWork={showStartWork}
              showConversationBinding={showConversationBinding}
              bindableConversations={bindableConversations}
              onBindConversation={onBindConversation}
              isBindPending={isBindPending}
              bindError={bindError}
              granolaEnabled={granolaEnabled}
              granolaProjectId={effectiveProjectId}
              granolaNotes={linkedGranolaNotes}
              availableGranolaNotes={availableGranolaNotes}
              isGranolaLoading={
                granolaSettingsQuery.isLoading ||
                granolaNotesQuery.isLoading ||
                granolaNotesQuery.isFetching
              }
              onBindGranolaNote={(input) => bindGranolaNote.mutate(input)}
              isGranolaBindPending={bindGranolaNote.isPending}
              granolaBindError={granolaBindError}
              onNavigate={onNavigate}
              onStartWork={onStartWork}
            />
          </div>
        ) : (
          <div className="p-6 text-sm text-[var(--text-muted)]">
            Loading ticket
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
}
