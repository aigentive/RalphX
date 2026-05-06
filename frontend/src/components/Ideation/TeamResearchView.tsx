/**
 * TeamResearchView - Collapsible artifact cards for the "Team Research" tab
 *
 * Cards collapsed by default with content_preview visible. Full artifact content
 * lazy-fetched on first expand and cached. Markdown rendering memoized.
 * macOS Tahoe styling, warm orange accent. Pattern: PlanDisplay.tsx Collapsible.
 */

import { useState, useCallback, memo } from "react";
import { Microscope, BarChart3, FileText, Loader2, ChevronDown } from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { artifactApi } from "@/api/artifact";
import type { Artifact } from "@/types/artifact";
import type { TeamArtifactSummary } from "@/api/team";
import { cn } from "@/lib/utils";
import { withAlpha } from "@/lib/theme-colors";

// ============================================================================
// Types
// ============================================================================

export interface TeamResearchViewProps {
  artifacts: TeamArtifactSummary[];
  sessionId: string;
}

interface ArtifactCardState {
  full: Artifact | null;
  loading: boolean;
  error: string | null;
}

// ============================================================================
// Icon Config (matches TeamArtifactChips pattern)
// ============================================================================

interface ArtifactTypeConfig {
  icon: React.ElementType;
  color: string;
}

const DEFAULT_CONFIG: ArtifactTypeConfig = { icon: FileText, color: "var(--text-muted)" };

const ARTIFACT_TYPE_CONFIG: Record<string, ArtifactTypeConfig> = {
  research: { icon: Microscope, color: "var(--accent-primary)" },
  analysis: { icon: BarChart3, color: "var(--status-info)" },
  summary: DEFAULT_CONFIG,
};

function getTypeConfig(artifactType: string): ArtifactTypeConfig {
  return ARTIFACT_TYPE_CONFIG[artifactType.toLowerCase()] ?? DEFAULT_CONFIG;
}

// ============================================================================
// Markdown Components (dark theme, matches PlanDisplay)
// ============================================================================

const markdownComponents = {
  a: ({ href, children, ...props }: React.AnchorHTMLAttributes<HTMLAnchorElement>) => (
    <a
      href={href}
      target="_blank"
      rel="noopener noreferrer"
      className="text-accent-primary hover:text-accent-primary/80 underline underline-offset-2 decoration-accent-primary/30 hover:decoration-accent-primary/60 transition-colors"
      {...props}
    >
      {children}
    </a>
  ),
  code: ({ className, children, ...props }: React.HTMLAttributes<HTMLElement>) => {
    const isBlock = className?.includes("language-");
    if (isBlock) {
      return (
        <code
          className={cn(
            "block p-3 rounded-md text-[0.8125rem] font-mono overflow-x-auto",
            "bg-white/[0.02] border border-white/[0.04]",
            className
          )}
          {...props}
        >
          {children}
        </code>
      );
    }
    return (
      <code
        className="px-1.5 py-0.5 rounded text-[0.8125rem] font-mono bg-white/[0.04] text-text-primary"
        {...props}
      >
        {children}
      </code>
    );
  },
  pre: ({ children, ...props }: React.HTMLAttributes<HTMLPreElement>) => (
    <pre className="my-3 rounded-md overflow-hidden" {...props}>
      {children}
    </pre>
  ),
  p: ({ children, ...props }: React.HTMLAttributes<HTMLParagraphElement>) => (
    <p className="mb-3 last:mb-0 leading-relaxed text-text-secondary" {...props}>
      {children}
    </p>
  ),
  ul: ({ children, ...props }: React.HTMLAttributes<HTMLUListElement>) => (
    <ul className="mb-3 space-y-1.5 pl-4" {...props}>
      {children}
    </ul>
  ),
  ol: ({ children, ...props }: React.HTMLAttributes<HTMLOListElement>) => (
    <ol className="list-decimal mb-3 space-y-1.5 pl-4" {...props}>
      {children}
    </ol>
  ),
  li: ({ children, ...props }: React.LiHTMLAttributes<HTMLLIElement>) => (
    <li className="text-text-secondary leading-relaxed relative before:content-['•'] before:absolute before:-left-3 before:text-accent-primary/50" {...props}>
      {children}
    </li>
  ),
  h1: ({ children, ...props }: React.HTMLAttributes<HTMLHeadingElement>) => (
    <h1 className="text-lg font-medium text-text-primary mb-3 mt-6 first:mt-0 pb-2 border-b border-white/[0.06]" {...props}>
      {children}
    </h1>
  ),
  h2: ({ children, ...props }: React.HTMLAttributes<HTMLHeadingElement>) => (
    <h2 className="text-base font-medium text-text-primary mb-2 mt-5" {...props}>
      {children}
    </h2>
  ),
  h3: ({ children, ...props }: React.HTMLAttributes<HTMLHeadingElement>) => (
    <h3 className="text-sm font-medium text-text-primary mb-2 mt-4" {...props}>
      {children}
    </h3>
  ),
  blockquote: ({ children, ...props }: React.HTMLAttributes<HTMLQuoteElement>) => (
    <blockquote
      className="border-l-2 border-accent-primary/30 pl-4 my-3 text-text-muted italic"
      {...props}
    >
      {children}
    </blockquote>
  ),
  hr: ({ ...props }: React.HTMLAttributes<HTMLHRElement>) => (
    <hr className="my-6 border-white/[0.06]" {...props} />
  ),
  table: ({ children, ...props }: React.TableHTMLAttributes<HTMLTableElement>) => (
    <div className="my-3 overflow-x-auto rounded-lg border border-white/[0.06]">
      <table className="w-full text-sm border-collapse" {...props}>
        {children}
      </table>
    </div>
  ),
  thead: ({ children, ...props }: React.HTMLAttributes<HTMLTableSectionElement>) => (
    <thead className="bg-white/[0.02]" {...props}>
      {children}
    </thead>
  ),
  tbody: ({ children, ...props }: React.HTMLAttributes<HTMLTableSectionElement>) => (
    <tbody {...props}>{children}</tbody>
  ),
  tr: ({ children, ...props }: React.HTMLAttributes<HTMLTableRowElement>) => (
    <tr className="border-b border-white/[0.06] last:border-b-0" {...props}>
      {children}
    </tr>
  ),
  th: ({ children, ...props }: React.ThHTMLAttributes<HTMLTableCellElement>) => (
    <th className="px-3 py-2 text-left text-xs font-medium text-text-primary uppercase tracking-wider" {...props}>
      {children}
    </th>
  ),
  td: ({ children, ...props }: React.TdHTMLAttributes<HTMLTableCellElement>) => (
    <td className="px-3 py-2 text-text-secondary" {...props}>
      {children}
    </td>
  ),
};

// ============================================================================
// Memoized Markdown Renderer (ReactMarkdown is expensive)
// ============================================================================

const MemoizedMarkdown = memo(function MemoizedMarkdown({ content }: { content: string }) {
  return (
    <ReactMarkdown remarkPlugins={[remarkGfm]} components={markdownComponents}>
      {content}
    </ReactMarkdown>
  );
});

// ============================================================================
// Artifact Card (collapsed by default, lazy-fetches on first expand)
// ============================================================================

function ArtifactCard({ artifact }: { artifact: TeamArtifactSummary }) {
  const [isOpen, setIsOpen] = useState(false);
  const [cardState, setCardState] = useState<ArtifactCardState>({
    full: null,
    loading: false,
    error: null,
  });
  const [hasFetched, setHasFetched] = useState(false);

  const config = getTypeConfig(artifact.artifact_type);
  const Icon = config.icon;

  const fullContent =
    cardState.full?.content.type === "inline" ? cardState.full.content.text : null;
  const author = cardState.full?.metadata.createdBy;

  const handleOpenChange = useCallback(
    (open: boolean) => {
      setIsOpen(open);

      // Lazy fetch: only on first expand
      if (open && !hasFetched) {
        setHasFetched(true);
        setCardState((prev) => ({ ...prev, loading: true }));

        artifactApi
          .get(artifact.id)
          .then((full) => {
            setCardState({ full, loading: false, error: null });
          })
          .catch((err) => {
            console.error(`Failed to fetch artifact ${artifact.id}:`, err);
            setCardState({ full: null, loading: false, error: "Failed to load content" });
          });
      }
    },
    [artifact.id, hasFetched],
  );

  return (
    <Collapsible open={isOpen} onOpenChange={handleOpenChange}>
      {/* Header - flat Tahoe style, visually separate from content */}
      <div
        className="rounded-xl transition-all duration-200"
        style={{
          padding: "12px 14px",
          background: isOpen
            ? withAlpha("var(--accent-primary)", 8)
            : "var(--overlay-faint)",
          border: isOpen
            ? "1px solid var(--accent-border)"
            : "1px solid var(--overlay-faint)",
        }}
      >
        <CollapsibleTrigger asChild>
          <button className="flex items-center gap-3 w-full text-left">
            {/* Type icon */}
            <div
              className="w-8 h-8 rounded-lg flex items-center justify-center flex-shrink-0 transition-all duration-200"
              style={{
                background: isOpen ? withAlpha(config.color, 15) : "var(--overlay-faint)",
                border: isOpen
                  ? `1px solid ${withAlpha(config.color, 30)}`
                  : "1px solid var(--overlay-faint)",
              }}
            >
              <Icon
                className="w-4 h-4 transition-colors duration-200"
                style={{ color: isOpen ? config.color : "var(--text-muted)" }}
              />
            </div>

            {/* Title, version badge, author */}
            <div className="flex-1 min-w-0">
              <div className="flex items-center gap-2">
                <span
                  className="text-[0.8125rem] font-medium truncate tracking-[-0.01em]"
                  style={{ color: "var(--text-primary)" }}
                >
                  {artifact.name}
                </span>
                <span
                  className="text-[0.625rem] font-medium px-1.5 py-0.5 rounded-md flex-shrink-0"
                  style={{
                    background: "var(--overlay-faint)",
                    border: "1px solid var(--overlay-faint)",
                    color: "var(--text-muted)",
                  }}
                >
                  v{artifact.version}
                </span>
              </div>

              {/* Collapsed: show author for scannable context */}
              {!isOpen && artifact.author_teammate && (
                <span
                  className="text-[0.6875rem] mt-0.5 block truncate"
                  style={{ color: "var(--text-muted)" }}
                >
                  by {artifact.author_teammate}
                </span>
              )}

              {/* Expanded: show author if available */}
              {isOpen && author && (
                <span
                  className="text-[0.6875rem] mt-0.5 block"
                  style={{ color: "var(--text-muted)" }}
                >
                  by {author}
                </span>
              )}
            </div>

            {/* Chevron toggle (matches PlanDisplay rotation) */}
            <ChevronDown
              className={cn(
                "w-4 h-4 transition-transform duration-200 flex-shrink-0",
                !isOpen && "-rotate-90",
              )}
              style={{ color: "var(--text-muted)" }}
            />
          </button>
        </CollapsibleTrigger>
      </div>

      {/* Content - renders BELOW header, visually separate (matches PlanDisplay) */}
      <CollapsibleContent>
        <div
          className="mt-3 pl-6 pr-2 pb-4"
          style={{
            marginLeft: "16px",
            borderLeft: `2px solid ${withAlpha("var(--accent-primary)", 15)}`,
          }}
        >
          {cardState.loading ? (
            <div className="flex items-center gap-2 py-4">
              <Loader2
                className="w-3.5 h-3.5 animate-spin"
                style={{ color: "var(--accent-primary)" }}
              />
              <span className="text-[0.6875rem]" style={{ color: "var(--text-muted)" }}>
                Loading full content...
              </span>
            </div>
          ) : cardState.error ? (
            <div className="py-4 text-center">
              <span className="text-[0.75rem]" style={{ color: "var(--status-error)" }}>
                {cardState.error}
              </span>
              <p
                className="text-[0.8125rem] leading-relaxed mt-2"
                style={{ color: "var(--text-secondary)" }}
              >
                {artifact.content_preview}
              </p>
            </div>
          ) : fullContent ? (
            <div className="text-[0.8125rem] leading-relaxed">
              <MemoizedMarkdown content={fullContent} />
            </div>
          ) : (
            <p
              className="text-[0.8125rem] italic py-4 text-center"
              style={{ color: "var(--text-muted)" }}
            >
              No content available
            </p>
          )}
        </div>
      </CollapsibleContent>
    </Collapsible>
  );
}

// ============================================================================
// Component
// ============================================================================

export function TeamResearchView({ artifacts }: TeamResearchViewProps) {
  // Empty state
  if (artifacts.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center py-16">
        <FileText className="w-10 h-10 mb-3" style={{ color: "var(--text-muted)" }} />
        <span className="text-[0.8125rem] font-medium" style={{ color: "var(--text-muted)" }}>
          No team research artifacts yet
        </span>
        <span className="text-[0.6875rem] mt-1" style={{ color: "var(--text-muted)" }}>
          Artifacts will appear here once team research completes
        </span>
      </div>
    );
  }

  return (
    <div className="space-y-3">
      {/* Section header */}
      <div className="flex items-center gap-2">
        <span
          className="text-[0.6875rem] font-medium tracking-wide uppercase"
          style={{ color: "var(--text-muted)" }}
        >
          Team Research
        </span>
        <span
          className="text-[0.625rem] font-medium px-1.5 py-0.5 rounded-md"
          style={{
            background: "var(--overlay-faint)",
            border: "1px solid var(--overlay-faint)",
            color: "var(--text-muted)",
          }}
        >
          {artifacts.length} artifact{artifacts.length !== 1 ? "s" : ""}
        </span>
      </div>

      {/* Collapsible artifact cards */}
      {artifacts.map((artifact) => (
        <ArtifactCard key={artifact.id} artifact={artifact} />
      ))}
    </div>
  );
}
