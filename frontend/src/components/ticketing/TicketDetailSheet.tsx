import { useEffect, useMemo, useRef, useState, type ComponentProps } from "react";
import { ExternalLink, FolderKanban, GitBranch, GitPullRequestArrow, Image as ImageIcon, MessageSquare, Send, UserCheck, UserX, X, ZoomIn } from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

import type {
  TicketAssociationItem,
  TicketAssociations,
  TicketAttachment,
  TicketComment,
  TicketDeepLink,
  TicketDetail,
  TicketingCapabilities,
  TicketLabelOption,
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
import { TicketAssigneeChips } from "./TicketAssigneeChip";
import { TicketLabelEditor } from "./TicketLabelEditor";
import { TicketLabels } from "./TicketLabels";
import { TicketSearchableSelect } from "./TicketSearchableSelect";
import { openExternalTicketUrl } from "./ticketing-open-external";
import { countNewComments, isCommentNewSince, sortCommentsByCreatedAt, ticketAssignees } from "./ticketing-read-state";
import { categoryToken, formatTicketDate, providerLabel, ticketCanonicalBranchName, ticketKey } from "./ticketing-utils";

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
  isLabelPending?: boolean | undefined;
  /** Selectable team labels for Linear pick-list mode. */
  labelOptions?: TicketLabelOption[] | undefined;
  isLabelOptionsLoading?: boolean | undefined;
  onTransitionTicket?: ((transition: TicketTransitionOption) => Promise<void> | void) | undefined;
  onAssignToMe?: (() => Promise<void> | void) | undefined;
  onClearAssignee?: (() => Promise<void> | void) | undefined;
  onAddComment?: ((bodyMarkdown: string) => Promise<void> | void) | undefined;
  onSetLabels?: ((labels: string[]) => Promise<void> | void) | undefined;
  /** Timestamp the viewer last opened this ticket; comments newer than it are "new". */
  seenUntil?: string | null | undefined;
  isStartWorkPending?: boolean | undefined;
  startWorkError?: string | null | undefined;
  /** Existing project conversations the viewer may bind to this ticket. */
  bindableConversations?: { id: string; title: string | null }[] | undefined;
  onBindConversation?: ((conversationId: string) => void) | undefined;
  isBindPending?: boolean | undefined;
  bindError?: string | null | undefined;
  onNavigate?: ((deepLink: TicketDeepLink) => void) | undefined;
  onStartWork?: (() => void) | undefined;
  /**
   * Whether the start-work + bind-conversation affordances are shown. Providers
   * without a conversation-link backend (e.g. ClickUp, deferred) pass `false` so
   * the non-functional affordance stays hidden. Defaults to `true`.
   */
  showConversationBinding?: boolean | undefined;
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
  leadingIcon,
}: {
  item: TicketAssociationItem;
  onNavigate?: ((deepLink: TicketDeepLink) => void) | undefined;
  leadingIcon?: React.ReactNode;
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
        <span className="flex min-w-0 items-center gap-1.5">
          {leadingIcon}
          <span className="truncate text-sm font-medium">{item.title}</span>
        </span>
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

/**
 * Inline picker that binds an existing project conversation to this ticket.
 * The toggle is local state so the first click paints synchronously without any
 * async work; selecting a row calls back and closes the picker.
 */
function BindConversationControl({
  conversations,
  onBindConversation,
  isBindPending = false,
  bindError,
}: {
  conversations: { id: string; title: string | null }[];
  onBindConversation?: ((conversationId: string) => void) | undefined;
  isBindPending?: boolean | undefined;
  bindError?: string | null | undefined;
}) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const filtered = useMemo(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) {
      return conversations;
    }
    return conversations.filter((conversation) =>
      (conversation.title ?? "").toLowerCase().includes(needle),
    );
  }, [conversations, query]);

  function handleBind(conversationId: string) {
    onBindConversation?.(conversationId);
    setOpen(false);
    setQuery("");
  }

  return (
    <div className="mt-2">
      <Button
        type="button"
        variant="ghost"
        size="sm"
        className="w-full justify-center"
        disabled={!onBindConversation || isBindPending}
        onClick={() => setOpen((current) => !current)}
        aria-expanded={open}
      >
        {isBindPending ? "Binding..." : "Bind existing conversation"}
      </Button>
      {open && (
        <div
          className="mt-2 rounded-md p-2"
          style={{
            backgroundColor: "var(--bg-surface)",
            borderColor: "var(--border-subtle)",
            borderStyle: "solid",
            borderWidth: "1px",
          }}
        >
          <input
            type="text"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Search conversations"
            aria-label="Search conversations"
            className="h-8 w-full rounded-md px-2 text-xs outline-none focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]"
            style={{
              backgroundColor: "var(--bg-elevated)",
              borderColor: "var(--border-subtle)",
              borderStyle: "solid",
              borderWidth: "1px",
              color: "var(--text-primary)",
            }}
          />
          <div className="mt-2 max-h-48 space-y-1 overflow-auto">
            {filtered.length === 0 ? (
              <p className="px-1 py-2 text-xs text-[var(--text-muted)]">
                No conversations to bind.
              </p>
            ) : (
              filtered.map((conversation) => (
                <button
                  key={conversation.id}
                  type="button"
                  className="w-full truncate rounded-md px-2 py-1.5 text-left text-xs hover:bg-[var(--bg-hover)] focus-visible:outline-none focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]"
                  style={{
                    backgroundColor: "var(--bg-elevated)",
                    borderColor: "var(--border-subtle)",
                    borderStyle: "solid",
                    borderWidth: "1px",
                    color: "var(--text-primary)",
                  }}
                  onClick={() => handleBind(conversation.id)}
                >
                  {conversation.title || "Untitled agent"}
                </button>
              ))
            )}
          </div>
        </div>
      )}
      {bindError && (
        <p className="mt-2 text-xs text-[var(--status-error)]" role="alert">
          {bindError}
        </p>
      )}
    </div>
  );
}

function RalphxAssociationPanel({
  ticket,
  associations,
  isLoading,
  isStartWorkPending = false,
  startWorkError,
  showConversationBinding = true,
  bindableConversations,
  onBindConversation,
  isBindPending,
  bindError,
  onNavigate,
  onStartWork,
}: {
  ticket: TicketDetail | TicketSummary | null;
  associations: TicketAssociations | undefined;
  isLoading: boolean;
  isStartWorkPending?: boolean | undefined;
  startWorkError?: string | null | undefined;
  showConversationBinding?: boolean | undefined;
  bindableConversations?: { id: string; title: string | null }[] | undefined;
  onBindConversation?: ((conversationId: string) => void) | undefined;
  isBindPending?: boolean | undefined;
  bindError?: string | null | undefined;
  onNavigate?: ((deepLink: TicketDeepLink) => void) | undefined;
  onStartWork?: (() => void) | undefined;
}) {
  const ticketBranchName = ticket ? ticketCanonicalBranchName(ticket.ref) : null;
  const hasTicketPr = Boolean(ticket?.openPrNumber);
  const hasTicketGitMetadata = Boolean(ticketBranchName || hasTicketPr);
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
        {isStartWorkPending
          ? "Starting..."
          : "Start conversation"}
      </Button>
      {startWorkError && (
        <p className="mt-2 text-xs text-[var(--status-error)]" role="alert">
          {startWorkError}
        </p>
      )}
      {showConversationBinding && (
        <>
          <BindConversationControl
            conversations={bindableConversations ?? []}
            onBindConversation={onBindConversation}
            isBindPending={isBindPending}
            bindError={bindError}
          />
        </>
      )}
      {isLoading ? (
        <p className="mt-4 text-sm text-[var(--text-muted)]">Loading associations</p>
      ) : (
        <div className="mt-4 min-h-0 space-y-4 overflow-auto">
          {hasTicketGitMetadata && (
            <section>
              <h4 className="mb-2 text-[11px] font-semibold uppercase text-[var(--text-muted)]">
                Ticket Git
              </h4>
              <div
                className="space-y-2 rounded-md p-3"
                style={{
                  backgroundColor: "var(--bg-elevated)",
                  borderColor: "var(--border-subtle)",
                  borderStyle: "solid",
                  borderWidth: "1px",
                }}
              >
                {ticketBranchName && (
                  <div>
                    <p className="text-[11px] font-medium uppercase text-[var(--text-muted)]">
                      Branch
                    </p>
                    <p className="mt-1 break-all font-mono text-xs text-[var(--text-primary)]">
                      {ticketBranchName}
                    </p>
                  </div>
                )}
                {hasTicketPr && (
                  <div>
                    <p className="text-[11px] font-medium uppercase text-[var(--text-muted)]">
                      Pull request
                    </p>
                    {ticket?.openPrUrl ? (
                      <button
                        type="button"
                        className="mt-1 inline-flex items-center gap-1.5 rounded text-xs font-medium text-[var(--status-info)] hover:underline focus-visible:outline-none focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]"
                        onClick={() => void openExternalTicketUrl(ticket.openPrUrl ?? "")}
                      >
                        <GitPullRequestArrow className="h-3.5 w-3.5" aria-hidden="true" />
                        PR #{ticket.openPrNumber}
                        {ticket.openPrStatus ? ` (${ticket.openPrStatus})` : ""}
                      </button>
                    ) : (
                      <p className="mt-1 text-xs text-[var(--text-primary)]">
                        PR #{ticket?.openPrNumber}
                        {ticket?.openPrStatus ? ` (${ticket.openPrStatus})` : ""}
                      </p>
                    )}
                  </div>
                )}
              </div>
            </section>
          )}
          {totalCount === 0 && (
            <p className="text-sm text-[var(--text-muted)]">
              No RalphX links yet. Start a conversation with this ticket attached.
            </p>
          )}
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
                  {items.map((item) => {
                    // In the Pull Requests bucket, distinguish a real PR from a
                    // branch-only workspace at a glance (the backend marks
                    // branch-only items with status "branch").
                    const prIcon =
                      group.key === "pullRequests"
                        ? item.status === "branch"
                          ? (
                              <span
                                role="img"
                                aria-label="Branch only (no pull request yet)"
                                title="Branch only (no pull request yet)"
                                className="inline-flex shrink-0 text-[var(--text-muted)]"
                              >
                                <GitBranch className="h-3.5 w-3.5" aria-hidden="true" />
                              </span>
                            )
                          : (
                              <span
                                role="img"
                                aria-label={
                                  item.active
                                    ? "Open pull request"
                                    : "Pull request (merged or closed)"
                                }
                                title={
                                  item.active
                                    ? "Open pull request"
                                    : "Pull request (merged or closed)"
                                }
                                className={
                                  item.active
                                    ? "inline-flex shrink-0 text-[var(--status-success)]"
                                    : "inline-flex shrink-0 text-[var(--text-muted)]"
                                }
                              >
                                <GitPullRequestArrow className="h-3.5 w-3.5" aria-hidden="true" />
                              </span>
                            )
                        : undefined;
                    return (
                      <AssociationCard
                        key={`${group.key}:${item.id}`}
                        item={item}
                        onNavigate={onNavigate}
                        {...(prIcon !== undefined && { leadingIcon: prIcon })}
                      />
                    );
                  })}
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

/**
 * Markdown image renderer for ticket content. Provider images (Jira/Linear) are
 * frequently authed/CORS-restricted and fail to load inline under WKWebView, which
 * would otherwise show a broken-image icon. On load failure we fall back to a
 * button that opens the image in the browser, where the user's session can fetch it.
 */
function TicketMarkdownImage({ src, alt }: ComponentProps<"img">) {
  const [failed, setFailed] = useState(false);
  const url = typeof src === "string" ? src : undefined;
  if (!url) {
    return null;
  }
  if (failed) {
    return (
      <button
        type="button"
        onClick={() => void openExternalTicketUrl(url)}
        className="inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-xs font-medium text-[var(--status-info)] hover:bg-[var(--bg-hover)] focus-visible:outline-none focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]"
        style={{
          borderColor: "var(--border-subtle)",
          borderStyle: "solid",
          borderWidth: "1px",
        }}
      >
        <ImageIcon className="h-3.5 w-3.5 shrink-0" aria-hidden="true" />
        {alt?.trim() ? alt : "View image"}
      </button>
    );
  }
  return (
    <img
      src={url}
      alt={alt ?? ""}
      loading="lazy"
      onError={() => setFailed(true)}
      className="max-w-full rounded"
    />
  );
}

function isImageAttachment(attachment: TicketAttachment): boolean {
  const mimeType = attachment.mimeType?.toLowerCase() ?? "";
  if (mimeType.startsWith("image/")) {
    return true;
  }
  return /\.(avif|gif|jpe?g|png|webp|svg)$/i.test(attachment.filename);
}

function formatAttachmentSize(size: number | null | undefined): string | null {
  if (typeof size !== "number" || !Number.isFinite(size) || size <= 0) {
    return null;
  }
  if (size < 1024) {
    return `${size} B`;
  }
  if (size < 1024 * 1024) {
    return `${(size / 1024).toFixed(1)} KB`;
  }
  return `${(size / (1024 * 1024)).toFixed(1)} MB`;
}

function TicketAttachmentPreview({
  attachment,
  compact = false,
}: {
  attachment: TicketAttachment;
  compact?: boolean;
}) {
  const [failed, setFailed] = useState(false);
  const [previewOpen, setPreviewOpen] = useState(false);
  const isImage = isImageAttachment(attachment);
  const sizeLabel = formatAttachmentSize(attachment.size);
  const canOpen = Boolean(attachment.url);
  const meta = [attachment.mimeType, sizeLabel].filter(Boolean).join(" · ");
  const canPreview = isImage && Boolean(attachment.url) && !failed;

  return (
    <>
      <article
        className={compact ? "flex overflow-hidden rounded-md" : "overflow-hidden rounded-md"}
        style={{
          backgroundColor: "var(--bg-surface)",
          borderColor: "var(--border-subtle)",
          borderStyle: "solid",
          borderWidth: "1px",
        }}
      >
        {canPreview && (
          <button
            type="button"
            className={[
              "group relative shrink-0 overflow-hidden bg-[var(--bg-elevated)] focus-visible:outline-none focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:-2px]",
              compact ? "h-16 w-20" : "block w-full",
            ].join(" ")}
            onClick={() => setPreviewOpen(true)}
            aria-label={`Preview attachment ${attachment.filename}`}
          >
            <img
              src={attachment.url ?? ""}
              alt={attachment.filename}
              loading="lazy"
              className={compact ? "h-full w-full object-cover" : "max-h-64 w-full object-contain"}
              onError={() => setFailed(true)}
            />
            <span className="absolute inset-0 flex items-center justify-center bg-[var(--overlay-scrim)] text-white opacity-0 transition-opacity group-hover:opacity-100 group-focus-visible:opacity-100">
              <ZoomIn className="h-5 w-5" aria-hidden="true" />
              <span className="sr-only">Preview image</span>
            </span>
          </button>
        )}
        <div
          className={[
            "flex min-w-0 items-center justify-between gap-3",
            compact ? "flex-1 p-2" : "p-3",
          ].join(" ")}
        >
          <div className="flex min-w-0 items-center gap-2">
            {!canPreview && (
              <ImageIcon className="h-4 w-4 shrink-0 text-[var(--text-muted)]" aria-hidden="true" />
            )}
            <div className="min-w-0">
              <p className="truncate text-sm font-medium text-[var(--text-primary)]">
                {attachment.filename}
              </p>
              {meta && <p className="mt-0.5 text-xs text-[var(--text-muted)]">{meta}</p>}
            </div>
          </div>
          {canOpen && (
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="h-7 shrink-0 gap-1 px-2 text-xs"
              onClick={() => void openExternalTicketUrl(attachment.url ?? "")}
            >
              <ExternalLink className="h-3.5 w-3.5" aria-hidden="true" />
              Open
            </Button>
          )}
        </div>
      </article>
      {canPreview && (
        <Dialog open={previewOpen} onOpenChange={setPreviewOpen}>
          <DialogContent
            className="max-w-5xl"
            style={{
              backgroundColor: "var(--bg-surface)",
              borderColor: "var(--border-subtle)",
              borderStyle: "solid",
              borderWidth: "1px",
            }}
          >
            <DialogHeader>
              <DialogTitle>{attachment.filename}</DialogTitle>
              {meta && <DialogDescription>{meta}</DialogDescription>}
            </DialogHeader>
            <div
              className="flex max-h-[75vh] items-center justify-center overflow-auto rounded-md p-2"
              style={{ backgroundColor: "var(--bg-elevated)" }}
            >
              <img
                src={attachment.url ?? ""}
                alt={attachment.filename}
                className="max-h-[72vh] max-w-full object-contain"
              />
            </div>
          </DialogContent>
        </Dialog>
      )}
    </>
  );
}

function TicketMarkdown({ content }: { content: string }) {
  return (
    <div className="prose prose-sm prose-invert max-w-none text-sm leading-6 text-[var(--text-secondary)] prose-code:before:content-none prose-code:after:content-none">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{ ...markdownComponents, img: TicketMarkdownImage }}
      >
        {content}
      </ReactMarkdown>
    </div>
  );
}

/** Animated skeleton placeholder shown while the ticket detail loads. */
function DetailSkeleton({ lines = 3 }: { lines?: number }) {
  return (
    <div className="mt-2 space-y-2" role="status" aria-label="Loading ticket details">
      {Array.from({ length: lines }).map((_, index) => (
        <div
          key={index}
          className="h-3 animate-pulse rounded"
          style={{
            backgroundColor: "var(--bg-surface)",
            width: index === lines - 1 ? "55%" : "100%",
          }}
        />
      ))}
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
  bindableConversations,
  onBindConversation,
  isBindPending,
  bindError,
  onNavigate,
  onStartWork,
  showConversationBinding = true,
  onClose,
}: TicketDetailSheetProps) {
  const [commentDraft, setCommentDraft] = useState("");
  const [localComments, setLocalComments] = useState<TicketComment[]>([]);
  const [expandedThreadIds, setExpandedThreadIds] = useState<Set<string>>(() => new Set());
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

  const sortedComments = useMemo(
    () => sortCommentsByCreatedAt(visibleComments),
    [visibleComments],
  );
  const newCommentCount = countNewComments(sortedComments, seenUntil ?? null);

  useEffect(() => {
    setLocalComments([]);
    setExpandedThreadIds(new Set());
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
  const assignees = ticket ? ticketAssignees(ticket) : [];
  const canClearAssignee = !assignDisabledReason && assignees.length > 0;
  const commentDisabledReason = !capabilities?.commentWrite
    ? "Comment write-back is not available for this provider."
    : null;
  const canAddComment = !commentDisabledReason && commentDraft.trim().length > 0 && !isCommentPending;
  const labelDisabledReason = !capabilities?.labelWrite
    ? "Label editing is not available for this provider."
    : null;
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
      attachments: [],
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

  function toggleThread(commentId: string) {
    setExpandedThreadIds((current) => {
      const next = new Set(current);
      if (next.has(commentId)) {
        next.delete(commentId);
      } else {
        next.add(commentId);
      }
      return next;
    });
  }

  return (
    <Dialog open={open} onOpenChange={(nextOpen) => {
      if (!nextOpen) {
        onClose();
      }
    }}>
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
                      style={newCommentCount > 0 ? { color: "var(--accent-primary)" } : undefined}
                    />
                    Comments ({sortedComments.length})
                    {newCommentCount > 0 && (
                      <span className="font-semibold" style={{ color: "var(--accent-primary)" }}>
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
                      <ExternalLink className="h-3.5 w-3.5" aria-hidden="true" />
                    </a>
                  )}
                </div>
                {(ticket.project || (!capabilities?.labelWrite && ticket.labels.length > 0)) && (
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
                      <TicketLabels labels={ticket.labels} max={ticket.labels.length} size="md" />
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
                        disabled={Boolean(statusDisabledReason) || isTransitionPending}
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
                              description: transition.disabledReason ?? undefined,
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
                              disabled={Boolean(assignDisabledReason) || isAssignPending}
                              onClick={() => void onClearAssignee?.()}
                              aria-label="Clear assignee"
                              title="Clear assignee"
                            >
                              <UserX className="h-3.5 w-3.5" aria-hidden="true" />
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
                          disabled={Boolean(assignDisabledReason) || isAssignPending}
                          onClick={() => void onAssignToMe?.()}
                        >
                          <UserCheck className="h-3.5 w-3.5" aria-hidden="true" />
                          {isAssignPending ? "Assigning" : "Assign to me"}
                        </Button>
                      </ControlTooltip>
                    )}
                  </div>
                </div>

                <section className="mt-5">
                  <h3 className="text-xs font-semibold uppercase text-[var(--text-muted)]">Description</h3>
                  <div className="mt-2">
                    {descriptionMarkdown ? (
                      <TicketMarkdown content={descriptionMarkdown} />
                    ) : isDetailLoading ? (
                      <DetailSkeleton lines={4} />
                    ) : (
                      <p className="text-sm leading-6 text-[var(--text-secondary)]">
                        No description provided.
                      </p>
                    )}
                  </div>
                </section>

                {"attachments" in ticket && ticket.attachments.length > 0 && (
                  <section className="mt-6">
                    <h3 className="text-xs font-semibold uppercase text-[var(--text-muted)]">
                      Attachments ({ticket.attachments.length})
                    </h3>
                    <div className="mt-2 grid gap-3 sm:grid-cols-2">
                      {ticket.attachments.map((attachment, index) => (
                        <TicketAttachmentPreview
                          key={attachment.id ?? `${attachment.filename}:${index}`}
                          attachment={attachment}
                        />
                      ))}
                    </div>
                  </section>
                )}

                <section ref={commentsSectionRef} className="mt-6">
                  <h3 className="text-xs font-semibold uppercase text-[var(--text-muted)]">
                    Comments ({sortedComments.length})
                    {newCommentCount > 0 && (
                      <span className="ml-1 normal-case" style={{ color: "var(--accent-primary)" }}>
                        · {newCommentCount} new
                      </span>
                    )}
                  </h3>
                  {sortedComments.length > 0 ? (
                    <div className="mt-2 space-y-2">
                      {sortedComments.map((comment, index) => {
                        const isNew = isCommentNewSince(comment, seenUntil ?? null);
                        const commentThreadId = comment.id ?? `comment-${index}`;
                        const threadOpen = expandedThreadIds.has(commentThreadId);
                        const replies = comment.replies ?? [];
                        const commentAttachments = comment.attachments ?? [];
                        return (
                          <article
                            key={commentThreadId}
                            className="rounded-md p-3"
                            style={{
                              backgroundColor: "var(--bg-surface)",
                              borderColor: isNew ? "var(--accent-border)" : "var(--border-subtle)",
                              borderStyle: "solid",
                              borderWidth: "1px",
                            }}
                          >
                            <div className="flex items-center justify-between gap-2">
                              <p className="text-xs font-medium text-[var(--text-secondary)]">
                                {comment.author?.name ?? "Provider comment"}
                              </p>
                              <div className="flex shrink-0 items-center gap-2">
                                {isNew && (
                                  <span
                                    className="inline-flex items-center gap-1 rounded-full px-1.5 py-0.5 text-[10px] font-semibold uppercase"
                                    style={{
                                      backgroundColor: "var(--accent-muted)",
                                      borderColor: "var(--accent-border)",
                                      borderStyle: "solid",
                                      borderWidth: "1px",
                                      color: "var(--accent-primary)",
                                    }}
                                  >
                                    New
                                  </span>
                                )}
                                {comment.createdAt && (
                                  <time
                                    className="text-[11px] text-[var(--text-muted)]"
                                    dateTime={comment.createdAt}
                                  >
                                    {formatTicketDate(comment.createdAt)}
                                  </time>
                                )}
                              </div>
                            </div>
                            <div className="mt-2">
                              <TicketMarkdown content={comment.bodyMarkdown || comment.bodyText} />
                            </div>
                            {commentAttachments.length > 0 && (
                              <div className="mt-3 grid gap-2 sm:grid-cols-2">
                                {commentAttachments.map((attachment, attachmentIndex) => (
                                  <TicketAttachmentPreview
                                    key={attachment.id ?? `${attachment.filename}:${attachmentIndex}`}
                                    attachment={attachment}
                                    compact
                                  />
                                ))}
                              </div>
                            )}
                            {replies.length > 0 && (
                              <div className="mt-3">
                                <Button
                                  type="button"
                                  variant="ghost"
                                  size="sm"
                                  className="h-7 gap-1.5 px-2 text-xs"
                                  onClick={() => toggleThread(commentThreadId)}
                                  aria-expanded={threadOpen}
                                >
                                  <MessageSquare className="h-3.5 w-3.5" aria-hidden="true" />
                                  {threadOpen
                                    ? `Hide thread (${replies.length})`
                                    : `View thread (${replies.length})`}
                                </Button>
                                {threadOpen && (
                                  <div className="mt-2 space-y-2 border-l pl-3" style={{ borderLeftColor: "var(--border-subtle)" }}>
                                    {replies.map((reply, replyIndex) => {
                                      const replyAttachments = reply.attachments ?? [];
                                      return (
                                        <article
                                          key={reply.id ?? `${commentThreadId}-reply-${replyIndex}`}
                                          className="rounded-md p-3"
                                          style={{
                                            backgroundColor: "var(--bg-elevated)",
                                            borderColor: "var(--border-subtle)",
                                            borderStyle: "solid",
                                            borderWidth: "1px",
                                          }}
                                        >
                                          <div className="flex items-center justify-between gap-2">
                                            <p className="text-xs font-medium text-[var(--text-secondary)]">
                                              {reply.author?.name ?? "Provider reply"}
                                            </p>
                                            {reply.createdAt && (
                                              <time
                                                className="text-[11px] text-[var(--text-muted)]"
                                                dateTime={reply.createdAt}
                                              >
                                                {formatTicketDate(reply.createdAt)}
                                              </time>
                                            )}
                                          </div>
                                          <div className="mt-2">
                                            <TicketMarkdown content={reply.bodyMarkdown || reply.bodyText} />
                                          </div>
                                          {replyAttachments.length > 0 && (
                                            <div className="mt-3 grid gap-2 sm:grid-cols-2">
                                              {replyAttachments.map((attachment, attachmentIndex) => (
                                                <TicketAttachmentPreview
                                                  key={attachment.id ?? `${attachment.filename}:${attachmentIndex}`}
                                                  attachment={attachment}
                                                  compact
                                                />
                                              ))}
                                            </div>
                                          )}
                                        </article>
                                      );
                                    })}
                                  </div>
                                )}
                              </div>
                            )}
                          </article>
                        );
                      })}
                    </div>
                  ) : isDetailLoading ? (
                    <DetailSkeleton lines={2} />
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
                      onKeyDown={(event) => {
                        // Cmd/Ctrl+Enter submits, matching the convention in
                        // Linear/GitHub/Slack comment composers.
                        if (
                          event.key === "Enter"
                          && (event.metaKey || event.ctrlKey)
                          && canAddComment
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
              showConversationBinding={showConversationBinding}
              bindableConversations={bindableConversations}
              onBindConversation={onBindConversation}
              isBindPending={isBindPending}
              bindError={bindError}
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
