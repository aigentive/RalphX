import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

import type { PullRequestDetail } from "@/api/github";
import { markdownComponents } from "@/components/Chat/MessageItem.markdown";
import { TicketMarkdownImage } from "@/components/ticketing/TicketDetailSheet";

export function DetailSkeleton({ lines = 3 }: { lines?: number }) {
  return (
    <div className="space-y-2" role="status" aria-label="Loading pull request">
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

export function normalizePrStatus(
  status: string | null | undefined,
  isDraft?: boolean,
): "Draft" | "Open" | "Merged" | "Closed" {
  if (isDraft) {
    return "Draft";
  }
  const normalized = status?.trim().toLowerCase();
  if (normalized === "merged") {
    return "Merged";
  }
  if (normalized === "closed") {
    return "Closed";
  }
  if (normalized === "draft") {
    return "Draft";
  }
  return "Open";
}

export function formatPrDate(value: string | null | undefined): string | null {
  if (!value) {
    return null;
  }
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return value;
  }
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  }).format(date);
}

export function PrMarkdown({ content }: { content: string }) {
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

export function PrSection({
  title,
  count,
  children,
}: {
  title: string;
  count?: number | undefined;
  children: React.ReactNode;
}) {
  return (
    <section className="space-y-3">
      <h3 className="text-xs font-semibold uppercase text-[var(--text-muted)]">
        {title}
        {count !== undefined ? ` (${count})` : ""}
      </h3>
      {children}
    </section>
  );
}

export function PrStateNotice({ state }: { state: PullRequestDetail["state"] }) {
  const copy: Record<PullRequestDetail["state"], string> = {
    loaded: "",
    noPr: "No pull request is linked to this branch.",
    ghUnauthenticated: "GitHub CLI is not authenticated. Run `gh auth login` and refresh.",
    repoUnresolvable: "RalphX could not resolve this repository on GitHub.",
    fetchTimeout: "GitHub did not return pull request details before the timeout.",
    rateLimited: "GitHub rate limited the pull request detail request.",
  };
  if (state === "loaded") {
    return null;
  }
  return (
    <div
      className="rounded-md px-3 py-3 text-sm text-[var(--text-secondary)]"
      style={{
        backgroundColor: "var(--bg-surface)",
        borderColor: "var(--border-subtle)",
        borderStyle: "solid",
        borderWidth: "1px",
      }}
    >
      {copy[state]}
    </div>
  );
}

export function PrCommentCard({
  author,
  createdAt,
  body,
  meta,
}: {
  author: string | null | undefined;
  createdAt: string | null | undefined;
  body: string;
  meta?: string | undefined;
}) {
  return (
    <article
      className="rounded-md p-3"
      style={{
        backgroundColor: "var(--bg-surface)",
        borderColor: "var(--border-subtle)",
        borderStyle: "solid",
        borderWidth: "1px",
      }}
    >
      <div className="flex items-center justify-between gap-2">
        <p className="truncate text-xs font-medium text-[var(--text-secondary)]">
          {author ?? "GitHub"}
          {meta ? <span className="text-[var(--text-muted)]"> · {meta}</span> : null}
        </p>
        {createdAt ? (
          <time className="shrink-0 text-[11px] text-[var(--text-muted)]" dateTime={createdAt}>
            {formatPrDate(createdAt)}
          </time>
        ) : null}
      </div>
      <div className="mt-2">
        <PrMarkdown content={body || "_No content._"} />
      </div>
    </article>
  );
}
