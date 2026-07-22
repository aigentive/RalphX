import {
  useEffect,
  useMemo,
  useState,
  type ComponentProps,
  type ReactNode,
  type RefObject,
} from "react";
import {
  ExternalLink,
  Image as ImageIcon,
  MessageSquare,
  ZoomIn,
} from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

import type {
  TicketAttachment,
  TicketComment,
  TicketDetail,
  TicketSummary,
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

import { openExternalTicketUrl } from "./ticketing-open-external";
import {
  countNewComments,
  isCommentNewSince,
  sortCommentsByCreatedAt,
} from "./ticketing-read-state";
import { formatTicketDate } from "./ticketing-utils";

/**
 * Markdown image renderer for ticket content. Provider images (Jira/Linear) are
 * frequently authed/CORS-restricted and fail to load inline under WKWebView, which
 * would otherwise show a broken-image icon. On load failure we fall back to a
 * button that opens the image in the browser, where the user's session can fetch it.
 */
export function TicketMarkdownImage({ src, alt }: ComponentProps<"img">) {
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
        className={
          compact
            ? "flex overflow-hidden rounded-md"
            : "overflow-hidden rounded-md"
        }
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
              className={
                compact
                  ? "h-full w-full object-cover"
                  : "max-h-64 w-full object-contain"
              }
              onError={() => setFailed(true)}
            />
            <span className="absolute inset-0 flex items-center justify-center bg-transparent text-[var(--text-on-scrim)] opacity-0 transition group-hover:bg-[var(--overlay-scrim)] group-hover:opacity-100 group-focus-visible:bg-[var(--overlay-scrim)] group-focus-visible:opacity-100">
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
              <ImageIcon
                className="h-4 w-4 shrink-0 text-[var(--text-muted)]"
                aria-hidden="true"
              />
            )}
            <div className="min-w-0">
              <p className="truncate text-sm font-medium text-[var(--text-primary)]">
                {attachment.filename}
              </p>
              {meta && (
                <p className="mt-0.5 text-xs text-[var(--text-muted)]">
                  {meta}
                </p>
              )}
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
    <div
      className="mt-2 space-y-2"
      role="status"
      aria-label="Loading ticket details"
    >
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

const EMPTY_TICKET_COMMENTS: TicketComment[] = [];

interface TicketDetailReadOnlyContentProps {
  ticket: TicketDetail | TicketSummary;
  comments?: TicketComment[];
  isDetailLoading?: boolean;
  seenUntil?: string | null;
  commentsSectionRef?: RefObject<HTMLElement | null>;
}

export function TicketDetailReadOnlyContent({
  ticket,
  comments,
  isDetailLoading = false,
  seenUntil = null,
  commentsSectionRef,
}: TicketDetailReadOnlyContentProps) {
  const [expandedThreadIds, setExpandedThreadIds] = useState<Set<string>>(
    () => new Set(),
  );
  const ticketIdentity = `${ticket.ref.provider}:${ticket.ref.id}`;
  const descriptionMarkdown =
    "descriptionMarkdown" in ticket
      ? (ticket.descriptionMarkdown ?? ticket.descriptionText ?? null)
      : null;
  const visibleComments = useMemo(
    () =>
      comments ??
      ("comments" in ticket ? ticket.comments : EMPTY_TICKET_COMMENTS),
    [comments, ticket],
  );
  const sortedComments = useMemo(
    () => sortCommentsByCreatedAt(visibleComments),
    [visibleComments],
  );
  const newCommentCount = countNewComments(sortedComments, seenUntil);

  useEffect(() => {
    setExpandedThreadIds(new Set());
  }, [ticketIdentity]);

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
    <>
      <section className="mt-5">
        <h3 className="text-xs font-semibold uppercase text-[var(--text-muted)]">
          Description
        </h3>
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
            <span
              className="ml-1 normal-case"
              style={{ color: "var(--accent-primary)" }}
            >
              · {newCommentCount} new
            </span>
          )}
        </h3>
        {sortedComments.length > 0 ? (
          <div className="mt-2 space-y-2">
            {sortedComments.map((comment, index) => {
              const commentThreadId = comment.id ?? `comment-${index}`;
              const replies = comment.replies ?? [];
              const threadOpen = expandedThreadIds.has(commentThreadId);
              return (
                <TicketCommentCard
                  key={commentThreadId}
                  comment={comment}
                  isNew={isCommentNewSince(comment, seenUntil)}
                  attachments={comment.attachments ?? []}
                >
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
                        <MessageSquare
                          className="h-3.5 w-3.5"
                          aria-hidden="true"
                        />
                        {threadOpen
                          ? `Hide thread (${replies.length})`
                          : `View thread (${replies.length})`}
                      </Button>
                      {threadOpen && (
                        <div
                          className="mt-2 space-y-2 border-l pl-3"
                          style={{ borderLeftColor: "var(--border-subtle)" }}
                        >
                          {replies.map((reply, replyIndex) => (
                            <TicketCommentCard
                              key={
                                reply.id ??
                                `${commentThreadId}-reply-${replyIndex}`
                              }
                              comment={reply}
                              isReply
                              attachments={reply.attachments ?? []}
                            />
                          ))}
                        </div>
                      )}
                    </div>
                  )}
                </TicketCommentCard>
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
    </>
  );
}

function TicketCommentCard({
  comment,
  attachments,
  isNew = false,
  isReply = false,
  children,
}: {
  comment: TicketComment;
  attachments: TicketAttachment[];
  isNew?: boolean;
  isReply?: boolean;
  children?: ReactNode;
}) {
  return (
    <article
      className="rounded-md p-3"
      style={{
        backgroundColor: isReply ? "var(--bg-elevated)" : "var(--bg-surface)",
        borderColor: isNew ? "var(--accent-border)" : "var(--border-subtle)",
        borderStyle: "solid",
        borderWidth: "1px",
      }}
    >
      <div className="flex items-center justify-between gap-2">
        <p className="text-xs font-medium text-[var(--text-secondary)]">
          {comment.author?.name ??
            (isReply ? "Provider reply" : "Provider comment")}
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
      {attachments.length > 0 && (
        <div className="mt-3 grid gap-2 sm:grid-cols-2">
          {attachments.map((attachment, index) => (
            <TicketAttachmentPreview
              key={attachment.id ?? `${attachment.filename}:${index}`}
              attachment={attachment}
              compact
            />
          ))}
        </div>
      )}
      {children}
    </article>
  );
}
