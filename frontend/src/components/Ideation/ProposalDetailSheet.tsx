/**
 * ProposalDetailSheet - Right-sliding detail panel for proposal inspection
 *
 * Design: Dark glass aesthetic with backdrop blur, warm orange accent
 */

import { useAgentGate } from "@/hooks/useAgentGate";
import React, { useEffect, useCallback, useMemo, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { X, FileEdit, Trash2, ExternalLink, CheckSquare } from "lucide-react";
import { Button } from "@/components/ui/button";
import { markdownComponents } from "@/components/Chat/MessageItem.markdown";
import { unescapeProposalText } from "@/lib/proposal-text";
import { withAlpha } from "@/lib/theme-colors";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import type { TaskProposal } from "@/types/ideation";
import type { DependencyDetail } from "./ProposalCard";
import { PRIORITY_CONFIG } from "./proposalPriority";

// ============================================================================
// Types
// ============================================================================

export interface ProposalDetailEnrichment {
  dependsOnDetails: DependencyDetail[];
  blocksCount: number;
  isOnCriticalPath: boolean;
}

export interface ProposalDetailSheetProps {
  proposal: TaskProposal | null;
  enrichment?: ProposalDetailEnrichment;
  isReadOnly?: boolean;
  onClose: () => void;
  onEdit?: (proposalId: string) => void;
  onDelete?: (proposalId: string) => void;
  onNavigateToTask?: (taskId: string) => void;
}

// ============================================================================
// Sub-components
// ============================================================================

function MarkdownText({
  text,
  className,
}: {
  text: string;
  className?: string;
}) {
  const decoded = useMemo(() => unescapeProposalText(text), [text]);
  return (
    <div
      className={`prose prose-sm prose-invert max-w-none prose-code:before:content-none prose-code:after:content-none ${className ?? ""}`.trim()}
    >
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={markdownComponents}>
        {decoded}
      </ReactMarkdown>
    </div>
  );
}

function MetadataChip({ label, value, accent }: { label: string; value: string; accent?: boolean }) {
  return (
    <div
      className="flex flex-col gap-0.5 px-3 py-2 rounded-lg"
      style={{
        background: accent ? withAlpha("var(--accent-primary)", 8) : "var(--overlay-faint)",
        border: accent ? "1px solid var(--accent-border)" : "1px solid var(--overlay-weak)",
      }}
    >
      <span className="text-[0.625rem] font-medium uppercase tracking-wider" style={{ color: "var(--text-muted)" }}>
        {label}
      </span>
      <span className="text-[0.75rem] font-medium" style={{ color: accent ? "var(--accent-primary)" : "var(--text-primary)" }}>
        {value}
      </span>
    </div>
  );
}

// ============================================================================
// Component
// ============================================================================

export const ProposalDetailSheet = React.memo(function ProposalDetailSheet({
  proposal,
  enrichment,
  isReadOnly = false,
  onClose,
  onEdit,
  onDelete,
  onNavigateToTask,
}: ProposalDetailSheetProps) {
  const agentGate = useAgentGate("proposalEdit");
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false);

  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    },
    [onClose]
  );

  useEffect(() => {
    if (!proposal) return;
    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [proposal, handleKeyDown]);

  if (!proposal) return null;

  const effectivePriority = proposal.userPriority ?? proposal.suggestedPriority;
  const config = PRIORITY_CONFIG[effectivePriority];

  const priorityColors = {
    critical: { bg: "var(--status-error-muted)", border: "var(--status-error-border)", text: "var(--status-error)" },
    high: { bg: withAlpha("var(--accent-primary)", 8), border: "var(--accent-border)", text: "var(--accent-primary)" },
    medium: { bg: "var(--status-warning-muted)", border: "var(--status-warning-border)", text: "var(--status-warning)" },
    low: { bg: withAlpha("var(--text-muted)", 8), border: withAlpha("var(--text-muted)", 20), text: "var(--text-muted)" },
  };
  const priorityColor = priorityColors[effectivePriority] || priorityColors.medium;

  const complexityLabel: Record<string, string> = {
    trivial: "Trivial",
    simple: "Simple",
    moderate: "Moderate",
    complex: "Complex",
    very_complex: "Very Complex",
  };

  return (
    <>
      {/* Backdrop */}
      <div
        data-testid="proposal-detail-backdrop"
        className="fixed inset-0 z-40"
        onClick={onClose}
        aria-hidden="true"
      />

      {/* Panel */}
      <div
        data-testid="proposal-detail-sheet"
        className="fixed top-0 right-0 h-full z-50 flex flex-col"
        style={{
          width: "420px",
          background: withAlpha("var(--bg-base)", 96),
          backdropFilter: "blur(20px) saturate(180%)",
          WebkitBackdropFilter: "blur(20px) saturate(180%)",
          borderLeft: "1px solid var(--overlay-weak)",
          boxShadow: "-4px 0 24px var(--overlay-scrim-med), -12px 0 48px var(--overlay-scrim)",
          animation: "slide-in-from-right 200ms ease-out both",
        }}
        role="dialog"
        aria-label="Proposal details"
        aria-modal="true"
      >
        {/* Header */}
        <div
          className="flex items-center justify-between px-5 py-4 flex-shrink-0"
          style={{ borderBottom: "1px solid var(--overlay-weak)" }}
        >
          <div className="flex items-center gap-2.5 min-w-0">
            <div
              className="w-1 h-5 rounded-full flex-shrink-0"
              style={{ background: "var(--accent-primary)" }}
            />
            <h2
              className="text-[0.8125rem] font-semibold truncate"
              style={{ color: "var(--text-primary)" }}
            >
              Proposal Details
            </h2>
          </div>
          <div className="flex items-center gap-1.5 flex-shrink-0">
            {!isReadOnly && onEdit && (
              <Button
                data-testid="edit-proposal-button"
                variant="ghost"
                size="icon"
                className="h-7 w-7 rounded-lg"
                style={{ color: "var(--text-muted)" }}
                onMouseEnter={(e) => { e.currentTarget.style.background = "var(--overlay-weak)"; e.currentTarget.style.color = "var(--text-secondary)"; }}
                onMouseLeave={(e) => { e.currentTarget.style.background = "transparent"; e.currentTarget.style.color = "var(--text-muted)"; }}
                disabled={agentGate.gated}
                onClick={() => { if (agentGate.gated) return; onEdit(proposal.id); }}
                title={agentGate.reason ?? "Edit proposal"}
              >
                <FileEdit className="w-3.5 h-3.5" />
              </Button>
            )}
            {!isReadOnly && onDelete && (
              <Button
                data-testid="delete-proposal-button"
                variant="ghost"
                size="icon"
                className="h-7 w-7 rounded-lg"
                style={{ color: "var(--text-muted)" }}
                onMouseEnter={(e) => { e.currentTarget.style.background = "var(--status-error-muted)"; e.currentTarget.style.color = "var(--status-error)"; }}
                onMouseLeave={(e) => { e.currentTarget.style.background = "transparent"; e.currentTarget.style.color = "var(--text-muted)"; }}
                onClick={() => setDeleteDialogOpen(true)}
                title="Delete proposal"
              >
                <Trash2 className="w-3.5 h-3.5" />
              </Button>
            )}
            <Button
              data-testid="close-sheet-button"
              variant="ghost"
              size="icon"
              className="h-7 w-7 rounded-lg"
              style={{ color: "var(--text-muted)" }}
              onMouseEnter={(e) => { e.currentTarget.style.background = "var(--overlay-weak)"; e.currentTarget.style.color = "var(--text-secondary)"; }}
              onMouseLeave={(e) => { e.currentTarget.style.background = "transparent"; e.currentTarget.style.color = "var(--text-muted)"; }}
              onClick={onClose}
              title="Close"
            >
              <X className="w-3.5 h-3.5" />
            </Button>
          </div>
        </div>

        {/* Scrollable Content */}
        <div className="flex-1 overflow-y-auto">
          <div className="px-5 py-5 space-y-6">
            {/* Title */}
            <div>
              <h3
                className="text-[0.9375rem] font-semibold leading-snug tracking-[-0.01em]"
                style={{ color: "var(--text-primary)" }}
              >
                {proposal.title}
              </h3>
              {enrichment?.isOnCriticalPath && (
                <span
                  className="inline-block mt-1.5 px-2 py-0.5 rounded text-[0.625rem] font-semibold uppercase tracking-wider"
                  style={{
                    background: withAlpha("var(--accent-primary)", 10),
                    border: "1px solid var(--accent-border)",
                    color: "var(--accent-primary)",
                  }}
                >
                  Critical Path
                </span>
              )}
            </div>

            {/* Description */}
            {proposal.description && (
              <div className="space-y-1.5">
                <span className="text-[0.6875rem] font-medium uppercase tracking-wider" style={{ color: "var(--text-muted)" }}>
                  Description
                </span>
                <div
                  className="text-[0.8125rem] leading-relaxed"
                  style={{ color: "var(--text-secondary)" }}
                >
                  <MarkdownText text={proposal.description} />
                </div>
              </div>
            )}

            {/* Metadata chips */}
            <div className="grid grid-cols-3 gap-2">
              <div
                className="flex flex-col gap-0.5 px-3 py-2 rounded-lg"
                style={{
                  background: priorityColor.bg,
                  border: `1px solid ${priorityColor.border}`,
                }}
              >
                <span className="text-[0.625rem] font-medium uppercase tracking-wider" style={{ color: "var(--text-muted)" }}>
                  Priority
                </span>
                <span className="text-[0.75rem] font-semibold" style={{ color: priorityColor.text }}>
                  {config.label}
                </span>
              </div>
              <MetadataChip label="Category" value={proposal.category} />
              <MetadataChip label="Complexity" value={complexityLabel[proposal.estimatedComplexity] ?? proposal.estimatedComplexity} />
            </div>

            {/* Priority Reason */}
            {proposal.priorityReason && (
              <div className="space-y-1.5">
                <span className="text-[0.6875rem] font-medium uppercase tracking-wider" style={{ color: "var(--text-muted)" }}>
                  Priority Rationale
                </span>
                <p
                  className="text-[0.75rem] leading-relaxed italic"
                  style={{ color: "var(--text-secondary)" }}
                >
                  "{proposal.priorityReason}"
                </p>
              </div>
            )}

            {/* Implementation Steps */}
            {proposal.steps.length > 0 && (
              <div className="space-y-2">
                <span className="text-[0.6875rem] font-medium uppercase tracking-wider" style={{ color: "var(--text-muted)" }}>
                  Implementation Steps
                </span>
                <ol className="space-y-2">
                  {proposal.steps.map((step, index) => (
                    <li key={index} className="flex items-start gap-3">
                      <span
                        className="flex-shrink-0 text-[0.6875rem] font-mono font-semibold mt-0.5 w-4 text-right"
                        style={{ color: "var(--accent-primary)" }}
                      >
                        {index + 1}.
                      </span>
                      <div
                        className="text-[0.8125rem] leading-snug min-w-0 flex-1"
                        style={{ color: "var(--text-secondary)" }}
                      >
                        <MarkdownText text={step} />
                      </div>
                    </li>
                  ))}
                </ol>
              </div>
            )}

            {/* Acceptance Criteria */}
            {proposal.acceptanceCriteria.length > 0 && (
              <div className="space-y-2">
                <span className="text-[0.6875rem] font-medium uppercase tracking-wider" style={{ color: "var(--text-muted)" }}>
                  Acceptance Criteria
                </span>
                <ul className="space-y-1.5">
                  {proposal.acceptanceCriteria.map((criterion, index) => (
                    <li key={index} className="flex items-start gap-2.5">
                      <CheckSquare
                        className="w-3.5 h-3.5 flex-shrink-0 mt-0.5"
                        style={{ color: withAlpha("var(--accent-primary)", 50) }}
                      />
                      <div
                        className="text-[0.8125rem] leading-snug min-w-0 flex-1"
                        style={{ color: "var(--text-secondary)" }}
                      >
                        <MarkdownText text={criterion} />
                      </div>
                    </li>
                  ))}
                </ul>
              </div>
            )}

            {/* Dependencies */}
            {enrichment && enrichment.dependsOnDetails.length > 0 && (
              <div className="space-y-2">
                <span className="text-[0.6875rem] font-medium uppercase tracking-wider" style={{ color: "var(--text-muted)" }}>
                  Depends On ({enrichment.dependsOnDetails.length})
                </span>
                <ul className="space-y-1.5">
                  {enrichment.dependsOnDetails.map((dep) => (
                    <li
                      key={dep.proposalId}
                      className="px-3 py-2 rounded-lg"
                      style={{
                        background: "var(--overlay-faint)",
                        border: "1px solid var(--overlay-weak)",
                      }}
                    >
                      <div className="text-[0.75rem] font-medium" style={{ color: "var(--text-secondary)" }}>
                        {dep.title}
                      </div>
                      {dep.reason && (
                        <div className="text-[0.6875rem] mt-0.5 italic" style={{ color: "var(--text-muted)" }}>
                          {dep.reason}
                        </div>
                      )}
                    </li>
                  ))}
                </ul>
              </div>
            )}

            {/* Blocks count */}
            {enrichment && enrichment.blocksCount > 0 && (
              <div
                className="flex items-center gap-2 px-3 py-2.5 rounded-lg"
                style={{
                  background: withAlpha("var(--accent-primary)", 6),
                  border: "1px solid var(--accent-muted)",
                }}
              >
                <span className="text-[0.75rem] font-semibold" style={{ color: "var(--accent-primary)" }}>
                  →{enrichment.blocksCount}
                </span>
                <span className="text-[0.75rem]" style={{ color: "var(--text-secondary)" }}>
                  Blocks {enrichment.blocksCount} proposal{enrichment.blocksCount !== 1 ? "s" : ""}
                </span>
              </div>
            )}

            {/* View Task link */}
            {proposal.createdTaskId && onNavigateToTask && (
              <button
                data-testid="view-task-from-detail"
                onClick={() => onNavigateToTask(proposal.createdTaskId!)}
                className="w-full flex items-center justify-center gap-2 px-4 py-2.5 rounded-lg text-[0.8125rem] font-medium transition-colors duration-150"
                style={{
                  background: withAlpha("var(--accent-primary)", 8),
                  border: "1px solid var(--accent-border)",
                  color: "var(--accent-primary)",
                }}
                onMouseEnter={(e) => { e.currentTarget.style.background = withAlpha("var(--accent-primary)", 12); }}
                onMouseLeave={(e) => { e.currentTarget.style.background = withAlpha("var(--accent-primary)", 8); }}
              >
                <ExternalLink className="w-3.5 h-3.5" />
                View Task →
              </button>
            )}
          </div>
        </div>
      </div>
      {onDelete && (
        <AlertDialog open={deleteDialogOpen} onOpenChange={setDeleteDialogOpen}>
          <AlertDialogContent>
            <AlertDialogHeader>
              <AlertDialogTitle>Delete Proposal</AlertDialogTitle>
              <AlertDialogDescription>
                Are you sure you want to delete "{proposal.title}"? This action cannot be undone.
              </AlertDialogDescription>
            </AlertDialogHeader>
            <AlertDialogFooter>
              <AlertDialogCancel>Cancel</AlertDialogCancel>
              <AlertDialogAction
                onClick={(e) => {
                  e.preventDefault();
                  onDelete(proposal.id);
                  onClose();
                }}
                className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
              >
                Delete
              </AlertDialogAction>
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialog>
      )}
    </>
  );
});
