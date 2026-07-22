import { useMemo, type CSSProperties, type ReactNode } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

import type { PullRequestDetail } from "@/api/github";
import { markdownComponents } from "@/components/Chat/MessageItem.markdown";
import { TicketMarkdownImage } from "@/components/ticketing/TicketDetailReadOnlyContent";

import { formatPrDate } from "./PullRequestDetailUtils";

const PR_LOADING_PLACEHOLDER_STYLE: CSSProperties = {
  backgroundColor: "var(--bg-hover)",
};

const PR_MARKDOWN_COMPONENTS = {
  ...markdownComponents,
  img: TicketMarkdownImage,
};

const DETAILS_TAG_RE = /<\/?details\b[^>]*>/gi;
const SUMMARY_TAG_RE = /<summary\b[^>]*>([\s\S]*?)<\/summary>/i;
const FENCE_LINE_RE = /^[ \t]{0,3}(```+|~~~+)/;

type MarkdownSegment =
  | { type: "markdown"; content: string }
  | { type: "details"; summary: string; body: string; open: boolean };

function decodeHtmlEntity(entity: string): string {
  const lower = entity.toLowerCase();
  const named: Record<string, string> = {
    amp: "&",
    apos: "'",
    gt: ">",
    lt: "<",
    nbsp: " ",
    quot: '"',
  };
  if (named[lower]) {
    return named[lower];
  }
  const decodeCodePoint = (codePoint: number): string =>
    Number.isInteger(codePoint) && codePoint >= 0 && codePoint <= 0x10ffff
      ? String.fromCodePoint(codePoint)
      : `&${entity};`;
  if (lower.startsWith("#x")) {
    const codePoint = Number.parseInt(lower.slice(2), 16);
    return decodeCodePoint(codePoint);
  }
  if (lower.startsWith("#")) {
    const codePoint = Number.parseInt(lower.slice(1), 10);
    return decodeCodePoint(codePoint);
  }
  return `&${entity};`;
}

function summaryTextWithoutMarkup(rawSummary: string): string {
  let text = "";
  let index = 0;

  while (index < rawSummary.length) {
    if (rawSummary.startsWith("<!--", index)) {
      const commentEnd = rawSummary.indexOf("-->", index + 4);
      index = commentEnd === -1 ? rawSummary.length : commentEnd + 3;
      continue;
    }

    const char = rawSummary[index];
    if (char === "<") {
      const tagEnd = rawSummary.indexOf(">", index + 1);
      if (tagEnd === -1) {
        text += char;
        index += 1;
        continue;
      }
      index = tagEnd + 1;
      continue;
    }

    text += char;
    index += 1;
  }

  return text;
}

function summaryText(rawSummary: string): string {
  const text = summaryTextWithoutMarkup(rawSummary)
    .replace(/&(#x?[0-9a-f]+|[a-z]+);/gi, (_, entity: string) =>
      decodeHtmlEntity(entity),
    )
    .replace(/\s+/g, " ")
    .trim();

  return text || "Details";
}

function hasOpenAttribute(openingTag: string): boolean {
  return /\sopen(?:\s|=|>|$)/i.test(openingTag);
}

function isBlockDetailsOpening(content: string, index: number): boolean {
  const lineStart = content.lastIndexOf("\n", index - 1) + 1;
  return /^[ \t]*$/.test(content.slice(lineStart, index));
}

function isInsideMarkdownFence(content: string, index: number): boolean {
  let inFence: { marker: string; length: number } | null = null;
  let lineStart = 0;

  while (lineStart < index) {
    const nextLineBreak = content.indexOf("\n", lineStart);
    const lineEnd =
      nextLineBreak === -1 || nextLineBreak > index ? index : nextLineBreak;
    const line = content.slice(lineStart, lineEnd);
    const match = FENCE_LINE_RE.exec(line);

    if (match) {
      const fence = match[1] ?? "";
      const marker = fence[0] ?? "";
      if (!inFence) {
        inFence = { marker, length: fence.length };
      } else if (marker === inFence.marker && fence.length >= inFence.length) {
        inFence = null;
      }
    }

    if (nextLineBreak === -1 || nextLineBreak >= index) {
      break;
    }
    lineStart = nextLineBreak + 1;
  }

  return inFence !== null;
}

function isOnIndentedCodeLine(content: string, index: number): boolean {
  const lineStart = content.lastIndexOf("\n", index - 1) + 1;
  return /^(?: {4,}|\t)/.test(content.slice(lineStart, index));
}

function isInsideMarkdownCodeSpan(content: string, index: number): boolean {
  const lineStart = content.lastIndexOf("\n", index - 1) + 1;
  const lineBeforeIndex = content.slice(lineStart, index);
  let activeBacktickLength: number | null = null;
  let cursor = 0;

  while (cursor < lineBeforeIndex.length) {
    if (lineBeforeIndex[cursor] !== "`") {
      cursor += 1;
      continue;
    }

    const runStart = cursor;
    while (cursor < lineBeforeIndex.length && lineBeforeIndex[cursor] === "`") {
      cursor += 1;
    }

    const runLength = cursor - runStart;
    if (activeBacktickLength === null) {
      activeBacktickLength = runLength;
    } else if (runLength === activeBacktickLength) {
      activeBacktickLength = null;
    }
  }

  return activeBacktickLength !== null;
}

function isInsideHtmlComment(content: string, index: number): boolean {
  return content.lastIndexOf("<!--", index) > content.lastIndexOf("-->", index);
}

function isInsideMarkdownLiteral(content: string, index: number): boolean {
  return (
    isInsideMarkdownFence(content, index) ||
    isOnIndentedCodeLine(content, index) ||
    isInsideMarkdownCodeSpan(content, index) ||
    isInsideHtmlComment(content, index)
  );
}

function findMatchingDetailsClose(
  content: string,
  searchFrom: number,
): { closeStart: number; closeEnd: number } | null {
  const tagRe = new RegExp(DETAILS_TAG_RE.source, "gi");
  tagRe.lastIndex = searchFrom;
  let depth = 1;
  let match: RegExpExecArray | null;

  while ((match = tagRe.exec(content)) !== null) {
    if (isInsideMarkdownLiteral(content, match.index)) {
      continue;
    }

    const tag = match[0];
    if (/^<\//.test(tag)) {
      depth -= 1;
      if (depth === 0) {
        return { closeStart: match.index, closeEnd: tagRe.lastIndex };
      }
    } else {
      depth += 1;
    }
  }

  return null;
}

function parseDetailsSegment(
  rawBlock: string,
  openingTag: string,
): MarkdownSegment | null {
  const summaryMatch = SUMMARY_TAG_RE.exec(rawBlock);
  if (!summaryMatch || summaryMatch.index === undefined) {
    return null;
  }
  if (/<details\b/i.test(rawBlock.slice(0, summaryMatch.index))) {
    return null;
  }

  const beforeSummary = rawBlock.slice(0, summaryMatch.index).trim();
  const afterSummary = rawBlock
    .slice(summaryMatch.index + summaryMatch[0].length)
    .trim();
  const body = [beforeSummary, afterSummary].filter(Boolean).join("\n\n");

  return {
    type: "details",
    summary: summaryText(summaryMatch[1] ?? ""),
    body,
    open: hasOpenAttribute(openingTag),
  };
}

function pushMarkdownSegment(
  segments: MarkdownSegment[],
  content: string,
): void {
  if (content.length === 0) {
    return;
  }
  segments.push({ type: "markdown", content });
}

function splitMarkdownSegments(content: string): MarkdownSegment[] {
  const segments: MarkdownSegment[] = [];
  const tagRe = new RegExp(DETAILS_TAG_RE.source, "gi");
  let cursor = 0;
  let match: RegExpExecArray | null;

  while ((match = tagRe.exec(content)) !== null) {
    const openingTag = match[0];
    if (/^<\//.test(openingTag)) {
      continue;
    }

    const openStart = match.index;
    const openEnd = tagRe.lastIndex;
    if (
      !isBlockDetailsOpening(content, openStart) ||
      isInsideMarkdownLiteral(content, openStart)
    ) {
      continue;
    }

    const close = findMatchingDetailsClose(content, openEnd);
    if (!close) {
      break;
    }

    const rawBlock = content.slice(openEnd, close.closeStart);
    const details = parseDetailsSegment(rawBlock, openingTag);
    if (!details) {
      tagRe.lastIndex = close.closeEnd;
      continue;
    }

    pushMarkdownSegment(segments, content.slice(cursor, openStart));
    segments.push(details);
    cursor = close.closeEnd;
    tagRe.lastIndex = close.closeEnd;
  }

  pushMarkdownSegment(segments, content.slice(cursor));
  return segments.length > 0 ? segments : [{ type: "markdown", content }];
}

function MarkdownContent({ content }: { content: string }) {
  return (
    <ReactMarkdown
      remarkPlugins={[remarkGfm]}
      components={PR_MARKDOWN_COMPONENTS}
    >
      {content}
    </ReactMarkdown>
  );
}

function PrDetailsBlock({
  summary,
  body,
  open,
}: {
  summary: string;
  body: string;
  open: boolean;
}) {
  return (
    <details
      open={open}
      className="my-3 rounded-md px-3 py-2"
      data-testid="pr-markdown-details"
      style={{
        backgroundColor: "var(--bg-surface)",
        borderColor: "var(--border-subtle)",
        borderStyle: "solid",
        borderWidth: "1px",
      }}
    >
      <summary className="cursor-pointer text-sm font-medium text-[var(--text-primary)]">
        {summary}
      </summary>
      {body.trim() ? (
        <div className="mt-3">
          <PrMarkdown content={body} />
        </div>
      ) : null}
    </details>
  );
}

export function DetailSkeleton({ lines = 3 }: { lines?: number }) {
  return (
    <div className="space-y-2" role="status" aria-label="Loading pull request">
      {Array.from({ length: lines }).map((_, index) => (
        <div
          key={index}
          data-testid="pr-detail-skeleton-line"
          className="h-3 animate-pulse rounded"
          style={{
            ...PR_LOADING_PLACEHOLDER_STYLE,
            width: index === lines - 1 ? "55%" : "100%",
          }}
        />
      ))}
    </div>
  );
}

export function PrMarkdown({ content }: { content: string }) {
  const segments = useMemo(() => splitMarkdownSegments(content), [content]);

  return (
    <div className="prose prose-sm prose-invert max-w-none text-sm leading-6 text-[var(--text-secondary)] prose-code:before:content-none prose-code:after:content-none">
      {segments.map((segment, index) =>
        segment.type === "details" ? (
          <PrDetailsBlock
            key={`${segment.summary}:${index}`}
            summary={segment.summary}
            body={segment.body}
            open={segment.open}
          />
        ) : (
          <MarkdownContent key={index} content={segment.content} />
        ),
      )}
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
  children: ReactNode;
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

export function PrStateNotice({
  state,
}: {
  state: PullRequestDetail["state"];
}) {
  const copy: Record<PullRequestDetail["state"], string> = {
    loaded: "",
    noPr: "No pull request is linked to this branch.",
    ghUnauthenticated:
      "GitHub CLI is not authenticated. Run `gh auth login` and refresh.",
    fetchUnavailable:
      "GitHub is temporarily unavailable. Recheck before signing in again.",
    repoUnresolvable: "RalphX could not resolve this repository on GitHub.",
    cliUnavailable:
      "GitHub CLI is unavailable. Install or configure gh, then refresh.",
    fetchTimeout:
      "GitHub did not return pull request details before the timeout.",
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
          {meta ? (
            <span className="text-[var(--text-muted)]"> · {meta}</span>
          ) : null}
        </p>
        {createdAt ? (
          <time
            className="shrink-0 text-[11px] text-[var(--text-muted)]"
            dateTime={createdAt}
          >
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
