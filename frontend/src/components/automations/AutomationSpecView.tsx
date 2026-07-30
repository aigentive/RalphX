import { Suspense, useMemo, useState } from "react";
import { ChevronDown, ChevronUp, FileText } from "lucide-react";

import { useAfterPaintMounted } from "@/components/agents/agentDeferredFrame";
import { ArtifactSelectableRegion } from "@/components/agents/artifact-selection/ArtifactSelectableRegion";
import { useArtifact } from "@/hooks/useArtifacts";
import { lazyWithRetry } from "@/lib/lazy-with-retry";

/**
 * Shared automation "Spec" body used by both the Agents automation panel and
 * the Automations detail view. The collapsed shell (name, excerpt, word count,
 * expand affordance) renders synchronously; the heavy Markdown renderer and its
 * dependencies are lazy-imported and only mounted after the user expands the
 * spec AND a paint boundary has passed. Collapsing defers teardown the same
 * way, so toggling never blocks the click commit.
 */

// react-markdown + remark-gfm + the chat markdown component overrides are all
// pulled into a single lazy chunk so none of them are evaluated until the spec
// is expanded.
const LazySpecMarkdown = lazyWithRetry(async () => {
  const [{ default: ReactMarkdown }, { default: remarkGfm }, { markdownComponents }] =
    await Promise.all([
      import("react-markdown"),
      import("remark-gfm"),
      import("@/components/Chat/MessageItem.markdown"),
    ]);

  return {
    default: function SpecMarkdown({ text }: { text: string }) {
      return (
        <ReactMarkdown remarkPlugins={[remarkGfm]} components={markdownComponents}>
          {text}
        </ReactMarkdown>
      );
    },
  };
});

const MARKDOWN_TOKEN_PREFIX = /^\s*(?:#{1,6}\s+|[-*+]\s+|>\s+|\d+\.\s+)/;
const MARKDOWN_LINK = /!?\[([^\]]+)\]\([^)]+\)/g;
const INLINE_MARKDOWN_TOKEN = /[*_~`]/g;

function buildExcerpt(content: string, maxLines = 3): string {
  const lines = content
    .split(/\r?\n/)
    .map((line) =>
      line
        .replace(MARKDOWN_TOKEN_PREFIX, "")
        .replace(MARKDOWN_LINK, "$1")
        .replace(INLINE_MARKDOWN_TOKEN, "")
        .trim(),
    )
    .filter((line) => line.length > 0);
  return lines.slice(0, maxLines).join("\n");
}

function countWords(content: string): number {
  const trimmed = content.trim();
  if (!trimmed) {
    return 0;
  }
  return trimmed.split(/\s+/).length;
}

function PlainSpecContent({ text }: { text: string }) {
  return (
    <p
      className="whitespace-pre-wrap break-words text-sm leading-6"
      style={{ color: "var(--text-secondary, #c7c7cc)" }}
    >
      {text}
    </p>
  );
}

export function AutomationSpecView({
  specArtifactId,
}: {
  specArtifactId: string | null;
}) {
  const [expanded, setExpanded] = useState(false);
  // Defer both the markdown mount (on expand) and teardown (on collapse) past a
  // paint boundary — the shell toggles synchronously.
  const markdownMounted = useAfterPaintMounted(expanded);

  // `useArtifact` no-ops on an empty id (`enabled: !!id`).
  const artifact = useArtifact(specArtifactId ?? "");
  const data = specArtifactId ? artifact.data : null;
  const isLoading = Boolean(specArtifactId) && artifact.isLoading;

  const content = data?.content ?? "";
  const excerpt = useMemo(() => buildExcerpt(content), [content]);
  const wordCount = useMemo(() => countWords(content), [content]);

  if (!data) {
    if (isLoading) {
      return (
        <p className="text-xs" style={{ color: "var(--text-muted, #8e8e96)" }}>
          Loading spec...
        </p>
      );
    }
    return (
      <p className="text-xs" style={{ color: "var(--text-muted, #8e8e96)" }}>
        No spec linked yet.
      </p>
    );
  }

  const hasContent = content.trim().length > 0;

  return (
    <ArtifactSelectableRegion
      className="space-y-3"
      source={{
        sourceKind: "automation_spec",
        sourceId: data.id,
        sourceLabel: "Automation spec",
        title: data.name,
        artifactId: data.id,
        version: data.version,
      }}
    >
      <p className="text-xs" style={{ color: "var(--text-muted, #8e8e96)" }}>
        The specification this automation implements.
      </p>
      <div className="flex items-start justify-between gap-3">
        <div className="flex min-w-0 items-center gap-2">
          <FileText
            className="h-4 w-4 shrink-0"
            style={{ color: "var(--text-muted, #8e8e96)" }}
            aria-hidden="true"
          />
          <span
            className="truncate text-sm font-medium"
            style={{ color: "var(--text-primary, #f2f2f4)" }}
          >
            {data.name}
          </span>
        </div>
        {hasContent ? (
          <span
            className="shrink-0 whitespace-nowrap text-xs tabular-nums"
            style={{ color: "var(--text-muted, #8e8e96)" }}
          >
            {wordCount} {wordCount === 1 ? "word" : "words"}
          </span>
        ) : null}
      </div>

      {!hasContent ? (
        <p className="text-xs" style={{ color: "var(--text-muted, #8e8e96)" }}>
          Spec has no content yet.
        </p>
      ) : expanded ? (
        <div
          data-testid="automation-spec-markdown"
          className="max-w-3xl text-sm leading-6 text-[var(--text-secondary)] [&>*+*]:mt-3"
        >
          {markdownMounted ? (
            <Suspense fallback={<PlainSpecContent text={content} />}>
              <LazySpecMarkdown text={content} />
            </Suspense>
          ) : (
            <PlainSpecContent text={content} />
          )}
        </div>
      ) : (
        <p
          className="line-clamp-3 whitespace-pre-line break-words text-sm leading-6"
          style={{ color: "var(--text-secondary, #c7c7cc)" }}
          data-testid="automation-spec-excerpt"
        >
          {excerpt}
        </p>
      )}

      {hasContent ? (
        <button
          type="button"
          className="inline-flex items-center gap-1 border-0 bg-transparent p-0 text-xs font-normal text-[var(--text-muted)] outline-none transition-colors hover:text-[var(--text-secondary)] focus-visible:ring-1 focus-visible:ring-[var(--accent-primary)]"
          aria-expanded={expanded}
          onClick={() => setExpanded((value) => !value)}
          data-testid="automation-spec-toggle"
        >
          {expanded ? (
            <ChevronUp className="h-3 w-3" aria-hidden="true" />
          ) : (
            <ChevronDown className="h-3 w-3" aria-hidden="true" />
          )}
          {expanded ? "Hide spec" : "Show full spec"}
        </button>
      ) : null}
    </ArtifactSelectableRegion>
  );
}
