/**
 * TeamArtifactDrawer - Slide-over inspector for team artifact content
 *
 * Design: macOS Tahoe Liquid Glass with warm orange accent.
 * Slides in from the right, replacing the chat panel.
 * Full markdown rendering with dark theme overrides.
 */

import { useEffect, useCallback, useRef, useState } from "react";
import {
  X,
  ChevronLeft,
  ChevronRight,
  FileText,
  Search,
  FlaskConical,
  MessageSquare,
  ClipboardList,
  Loader2,
  User,
} from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { cn } from "@/lib/utils";
import { withAlpha } from "@/lib/theme-colors";
import type { Artifact } from "@/types/artifact";
import type { TeamArtifactSummary } from "@/api/team";

// ============================================================================
// Types
// ============================================================================

export interface TeamArtifactDrawerProps {
  artifact: Artifact | null;
  allArtifacts: TeamArtifactSummary[];
  selectedIndex: number;
  isLoading: boolean;
  onClose: () => void;
  onNavigate: (artifactId: string) => void;
}

// ============================================================================
// Helpers
// ============================================================================

function ArtifactTypeIcon({ type, className, style }: { type: string; className?: string; style?: React.CSSProperties }) {
  const props = { className, style };
  switch (type) {
    case "research_document":
    case "research_brief":
      return <Search {...props} />;
    case "findings":
    case "recommendations":
      return <FlaskConical {...props} />;
    case "review_feedback":
    case "approval":
      return <MessageSquare {...props} />;
    case "task_spec":
    case "specification":
      return <ClipboardList {...props} />;
    default:
      return <FileText {...props} />;
  }
}

function formatArtifactType(type: string): string {
  return type
    .split("_")
    .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
    .join(" ");
}

function formatTimestamp(iso: string): string {
  try {
    const d = new Date(iso);
    return d.toLocaleDateString(undefined, {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  } catch {
    return iso;
  }
}

// ============================================================================
// Markdown Components - Dark theme overrides (matches PlanDisplay)
// ============================================================================

const markdownComponents = {
  a: ({
    href,
    children,
    ...props
  }: React.AnchorHTMLAttributes<HTMLAnchorElement>) => (
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
  code: ({
    className,
    children,
    ...props
  }: React.HTMLAttributes<HTMLElement>) => {
    const isBlock = className?.includes("language-");
    if (isBlock) {
      return (
        <code
          className={cn(
            "block p-3 rounded-md text-[0.8125rem] font-mono overflow-x-auto",
            "bg-white/[0.02] border border-white/[0.04]",
            className,
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
    <p
      className="mb-3 last:mb-0 leading-relaxed text-text-secondary"
      {...props}
    >
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
    <li
      className="text-text-secondary leading-relaxed relative before:content-['•'] before:absolute before:-left-3 before:text-accent-primary/50"
      {...props}
    >
      {children}
    </li>
  ),
  h1: ({ children, ...props }: React.HTMLAttributes<HTMLHeadingElement>) => (
    <h1
      className="text-lg font-medium text-text-primary mb-3 mt-6 first:mt-0 pb-2 border-b border-white/[0.06]"
      {...props}
    >
      {children}
    </h1>
  ),
  h2: ({ children, ...props }: React.HTMLAttributes<HTMLHeadingElement>) => (
    <h2
      className="text-base font-medium text-text-primary mb-2 mt-5"
      {...props}
    >
      {children}
    </h2>
  ),
  h3: ({ children, ...props }: React.HTMLAttributes<HTMLHeadingElement>) => (
    <h3
      className="text-sm font-medium text-text-primary mb-2 mt-4"
      {...props}
    >
      {children}
    </h3>
  ),
  blockquote: ({
    children,
    ...props
  }: React.HTMLAttributes<HTMLQuoteElement>) => (
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
  table: ({
    children,
    ...props
  }: React.TableHTMLAttributes<HTMLTableElement>) => (
    <div className="my-3 overflow-x-auto rounded-lg border border-white/[0.06]">
      <table className="w-full text-sm border-collapse" {...props}>
        {children}
      </table>
    </div>
  ),
  thead: ({
    children,
    ...props
  }: React.HTMLAttributes<HTMLTableSectionElement>) => (
    <thead className="bg-white/[0.02]" {...props}>
      {children}
    </thead>
  ),
  tbody: ({
    children,
    ...props
  }: React.HTMLAttributes<HTMLTableSectionElement>) => (
    <tbody {...props}>{children}</tbody>
  ),
  tr: ({ children, ...props }: React.HTMLAttributes<HTMLTableRowElement>) => (
    <tr
      className="border-b border-white/[0.06] last:border-b-0"
      {...props}
    >
      {children}
    </tr>
  ),
  th: ({
    children,
    ...props
  }: React.ThHTMLAttributes<HTMLTableCellElement>) => (
    <th
      className="px-3 py-2 text-left text-xs font-medium text-text-primary uppercase tracking-wider"
      {...props}
    >
      {children}
    </th>
  ),
  td: ({
    children,
    ...props
  }: React.TdHTMLAttributes<HTMLTableCellElement>) => (
    <td className="px-3 py-2 text-text-secondary" {...props}>
      {children}
    </td>
  ),
};

// ============================================================================
// Component
// ============================================================================

export function TeamArtifactDrawer({
  artifact,
  allArtifacts,
  selectedIndex,
  isLoading,
  onClose,
  onNavigate,
}: TeamArtifactDrawerProps) {
  const [isVisible, setIsVisible] = useState(false);
  const [isClosing, setIsClosing] = useState(false);
  const panelRef = useRef<HTMLDivElement>(null);

  // Determine open state from props
  const isOpen = artifact !== null || isLoading;

  // Trigger enter animation
  useEffect(() => {
    if (isOpen) {
      // Force a frame so the initial translateX(100%) renders before transition
      requestAnimationFrame(() => {
        setIsVisible(true);
      });
      setIsClosing(false);
    }
  }, [isOpen]);

  // Close with exit animation
  const handleClose = useCallback(() => {
    setIsClosing(true);
    setIsVisible(false);
    const timeout = setTimeout(() => {
      setIsClosing(false);
      onClose();
    }, 150);
    return () => clearTimeout(timeout);
  }, [onClose]);

  // Keyboard: Escape closes, arrow keys navigate
  useEffect(() => {
    if (!isOpen) return;

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        handleClose();
      } else if (e.key === "ArrowLeft" && selectedIndex > 0) {
        const prev = allArtifacts[selectedIndex - 1];
        if (prev) {
          e.preventDefault();
          onNavigate(prev.id);
        }
      } else if (
        e.key === "ArrowRight" &&
        selectedIndex < allArtifacts.length - 1
      ) {
        const next = allArtifacts[selectedIndex + 1];
        if (next) {
          e.preventDefault();
          onNavigate(next.id);
        }
      }
    };

    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [isOpen, handleClose, selectedIndex, allArtifacts, onNavigate]);

  if (!isOpen && !isClosing) return null;

  const currentSummary = allArtifacts[selectedIndex];

  const content =
    artifact?.content.type === "inline" ? artifact.content.text : "";

  const hasPrev = selectedIndex > 0;
  const hasNext = selectedIndex < allArtifacts.length - 1;

  return (
    <div
      ref={panelRef}
      className="flex flex-col h-full overflow-hidden"
      style={{
        borderLeft: `2px solid ${withAlpha("var(--accent-primary)", 30)}`,
        background: "var(--bg-surface)",
        transform: isVisible && !isClosing ? "translateX(0)" : "translateX(100%)",
        transition: isClosing
          ? "transform 150ms ease-in"
          : "transform 200ms ease-out",
      }}
      data-testid="team-artifact-drawer"
    >
      {/* Header */}
      <header
        className="flex items-center gap-3 px-4 h-12 shrink-0 border-b"
        style={{
          borderColor: "var(--overlay-faint)",
          background: withAlpha("var(--bg-surface)", 85),
          backdropFilter: "blur(20px)",
          WebkitBackdropFilter: "blur(20px)",
        }}
      >
        {/* Type icon */}
        <div
          className="w-7 h-7 rounded-lg flex items-center justify-center shrink-0"
          style={{
            background: withAlpha("var(--accent-primary)", 12),
            border: "1px solid var(--accent-border)",
          }}
        >
          <ArtifactTypeIcon
            type={currentSummary?.artifact_type ?? "default"}
            className="w-3.5 h-3.5"
            style={{ color: "var(--accent-primary)" }}
          />
        </div>

        {/* Title + meta */}
        <div className="flex-1 min-w-0">
          <h2
            className="text-[0.8125rem] font-medium truncate tracking-[-0.01em]"
            style={{ color: "var(--text-primary)" }}
          >
            {currentSummary?.name ?? "Artifact"}
          </h2>
          <div className="flex items-center gap-2">
            {currentSummary && (
              <span
                className="text-[0.625rem] font-medium"
                style={{ color: "var(--text-muted)" }}
              >
                {formatArtifactType(currentSummary.artifact_type)}
              </span>
            )}
            {artifact?.metadata.createdBy && (
              <span className="flex items-center gap-1">
                <User
                  className="w-2.5 h-2.5"
                  style={{ color: "var(--text-muted)" }}
                />
                <span
                  className="text-[0.625rem]"
                  style={{ color: "var(--text-muted)" }}
                >
                  {artifact.metadata.createdBy}
                </span>
              </span>
            )}
          </div>
        </div>

        {/* Version badge */}
        {currentSummary && (
          <span
            className="text-[0.625rem] font-medium px-1.5 py-0.5 rounded-md shrink-0"
            style={{
              background: "var(--overlay-faint)",
              border: "1px solid var(--overlay-faint)",
              color: "var(--text-muted)",
            }}
          >
            v{currentSummary.version}
          </span>
        )}

        {/* Timestamp */}
        {currentSummary && (
          <span
            className="text-[0.625rem] shrink-0 hidden sm:inline"
            style={{ color: "var(--text-muted)" }}
          >
            {formatTimestamp(currentSummary.created_at)}
          </span>
        )}

        {/* Close button */}
        <button
          onClick={handleClose}
          className="w-7 h-7 rounded-lg flex items-center justify-center shrink-0 transition-colors duration-150"
          style={{ color: "var(--text-muted)" }}
          onMouseEnter={(e) => {
            e.currentTarget.style.background = "var(--overlay-weak)";
            e.currentTarget.style.color = "var(--text-primary)";
          }}
          onMouseLeave={(e) => {
            e.currentTarget.style.background = "transparent";
            e.currentTarget.style.color = "var(--text-muted)";
          }}
          aria-label="Close drawer"
        >
          <X className="w-4 h-4" />
        </button>
      </header>

      {/* Body */}
      <div className="flex-1 overflow-y-auto p-5">
        {isLoading ? (
          <div className="flex items-center justify-center py-16">
            <div className="flex flex-col items-center gap-3">
              <Loader2
                className="w-6 h-6 animate-spin"
                style={{ color: "var(--accent-primary)" }}
              />
              <span
                className="text-[0.75rem]"
                style={{ color: "var(--text-muted)" }}
              >
                Loading artifact...
              </span>
            </div>
          </div>
        ) : content ? (
          <div className="text-[0.8125rem] leading-relaxed">
            <ReactMarkdown
              remarkPlugins={[remarkGfm]}
              components={markdownComponents}
            >
              {content}
            </ReactMarkdown>
          </div>
        ) : (
          <p
            className="text-[0.8125rem] italic py-8 text-center"
            style={{ color: "var(--text-muted)" }}
          >
            No content available
          </p>
        )}
      </div>

      {/* Footer - prev/next navigation */}
      {allArtifacts.length > 1 && (
        <footer
          className="flex items-center justify-between px-4 h-10 shrink-0 border-t"
          style={{
            borderColor: "var(--overlay-faint)",
            background: withAlpha("var(--bg-surface)", 85),
            backdropFilter: "blur(20px)",
            WebkitBackdropFilter: "blur(20px)",
          }}
        >
          <button
            onClick={() => {
              const prev = allArtifacts[selectedIndex - 1];
              if (hasPrev && prev) onNavigate(prev.id);
            }}
            disabled={!hasPrev}
            className={cn(
              "flex items-center gap-1.5 px-2.5 py-1 rounded-md text-[0.6875rem] font-medium transition-colors duration-150",
              hasPrev
                ? "cursor-pointer"
                : "cursor-not-allowed opacity-30",
            )}
            style={{
              color: hasPrev ? "var(--text-secondary)" : "var(--text-muted)",
            }}
            onMouseEnter={(e) => {
              if (hasPrev) {
                e.currentTarget.style.background = "var(--overlay-weak)";
                e.currentTarget.style.color = "var(--text-primary)";
              }
            }}
            onMouseLeave={(e) => {
              e.currentTarget.style.background = "transparent";
              e.currentTarget.style.color = hasPrev
                ? "var(--text-secondary)"
                : "var(--text-muted)";
            }}
            aria-label="Previous artifact"
          >
            <ChevronLeft className="w-3.5 h-3.5" />
            Prev
          </button>

          <span
            className="text-[0.625rem] font-medium"
            style={{ color: "var(--text-muted)" }}
          >
            {selectedIndex + 1} / {allArtifacts.length}
          </span>

          <button
            onClick={() => {
              const next = allArtifacts[selectedIndex + 1];
              if (hasNext && next) onNavigate(next.id);
            }}
            disabled={!hasNext}
            className={cn(
              "flex items-center gap-1.5 px-2.5 py-1 rounded-md text-[0.6875rem] font-medium transition-colors duration-150",
              hasNext
                ? "cursor-pointer"
                : "cursor-not-allowed opacity-30",
            )}
            style={{
              color: hasNext ? "var(--text-secondary)" : "var(--text-muted)",
            }}
            onMouseEnter={(e) => {
              if (hasNext) {
                e.currentTarget.style.background = "var(--overlay-weak)";
                e.currentTarget.style.color = "var(--text-primary)";
              }
            }}
            onMouseLeave={(e) => {
              e.currentTarget.style.background = "transparent";
              e.currentTarget.style.color = hasNext
                ? "var(--text-secondary)"
                : "var(--text-muted)";
            }}
            aria-label="Next artifact"
          >
            Next
            <ChevronRight className="w-3.5 h-3.5" />
          </button>
        </footer>
      )}
    </div>
  );
}
