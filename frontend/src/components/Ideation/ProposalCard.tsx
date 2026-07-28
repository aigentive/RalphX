/**
 * ProposalCard - macOS Tahoe styled proposal card
 *
 * Design: Glass-morphism card with refined shadows, smooth animations,
 * and warm orange accent for selection states.
 */

import { useAgentGate } from "@/hooks/useAgentGate";
import React, { useState } from "react";
import { Button } from "@/components/ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { FileEdit, Eye, ChevronDown, ChevronRight, ExternalLink, Trash2 } from "lucide-react";
import { cn } from "@/lib/utils";
import { withAlpha } from "@/lib/theme-colors";
import { buildProposalPreview } from "@/lib/proposal-text";
import type { TaskProposal } from "@/types/ideation";
import { PRIORITY_CONFIG } from "./proposalPriority";

// ============================================================================
// Types
// ============================================================================

export interface DependencyDetail {
  proposalId: string;
  title: string;
  reason?: string;
}

export interface ProposalCardProps {
  proposal: TaskProposal;
  onEdit: (proposalId: string) => void;
  isHighlighted?: boolean;
  currentPlanVersion?: number | undefined;
  onViewHistoricalPlan?: (artifactId: string, version: number) => void | undefined;
  dependsOnCount?: number;
  dependsOnDetails?: DependencyDetail[];
  blocksCount?: number;
  isOnCriticalPath?: boolean;
  /** Whether the plan is in read-only mode (accepted/archived status) */
  isReadOnly?: boolean;
  /** Callback to navigate to the created task in kanban */
  onNavigateToTask?: (taskId: string) => void;
  /** Callback when card body is clicked to open detail sheet */
  onViewDetail?: (proposalId: string, enrichment: { dependsOnDetails: DependencyDetail[]; blocksCount: number; isOnCriticalPath: boolean }) => void;
  /** Whether this card is currently selected (detail sheet open) */
  isSelected?: boolean;
  /** Callback to delete this proposal (triggers confirmation in parent) */
  onDelete?: (proposalId: string) => void;
}

// ============================================================================
// Component
// ============================================================================

const MAX_INLINE_DEPS = 2;

export const ProposalCard = React.memo(function ProposalCard({
  proposal,
  onEdit,
  isHighlighted = false,
  currentPlanVersion,
  onViewHistoricalPlan,
  dependsOnCount: _dependsOnCount,
  dependsOnDetails,
  blocksCount,
  isOnCriticalPath,
  isReadOnly = false,
  onNavigateToTask,
  onViewDetail,
  isSelected = false,
  onDelete,
}: ProposalCardProps) {
  const agentGate = useAgentGate();
  const [isDepsExpanded, setIsDepsExpanded] = useState(false);
  const [isHovered, setIsHovered] = useState(false);
  const effectivePriority = proposal.userPriority ?? proposal.suggestedPriority;
  const config = PRIORITY_CONFIG[effectivePriority];

  const hasDependencies = dependsOnDetails && dependsOnDetails.length > 0;
  const visibleDeps = hasDependencies ? dependsOnDetails.slice(0, MAX_INLINE_DEPS) : [];
  const overflowCount = hasDependencies ? Math.max(0, dependsOnDetails.length - MAX_INLINE_DEPS) : 0;
  const inlineText = visibleDeps.map((d) => d.title).join(", ");

  const showHistoricalPlanLink =
    proposal.planArtifactId &&
    proposal.planVersionAtCreation &&
    currentPlanVersion &&
    proposal.planVersionAtCreation !== currentPlanVersion;

  const handleViewHistoricalPlan = () => {
    if (proposal.planArtifactId && proposal.planVersionAtCreation && onViewHistoricalPlan) {
      onViewHistoricalPlan(proposal.planArtifactId, proposal.planVersionAtCreation);
    }
  };

  // Priority-based accent colors — design-token form.
  const priorityColors = {
    critical: { bg: "var(--status-error-muted)", border: "var(--status-error-border)", text: "var(--status-error)" },
    high: { bg: withAlpha("var(--accent-primary)", 8), border: "var(--accent-border)", text: "var(--accent-primary)" },
    medium: { bg: "var(--status-warning-muted)", border: "var(--status-warning-border)", text: "var(--status-warning)" },
    low: { bg: withAlpha("var(--text-muted)", 8), border: withAlpha("var(--text-muted)", 20), text: "var(--text-muted)" },
  };

  const priorityColor = priorityColors[effectivePriority] || priorityColors.medium;

  const handleCardClick = () => {
    if (onViewDetail) {
      onViewDetail(proposal.id, {
        dependsOnDetails: dependsOnDetails ?? [],
        blocksCount: blocksCount ?? 0,
        isOnCriticalPath: isOnCriticalPath ?? false,
      });
    }
  };

  return (
    <div
      data-testid={`proposal-card-${proposal.id}`}
      className={cn(
        "group relative rounded-xl",
        "transition-all duration-200 ease-out",
        onViewDetail ? "cursor-pointer" : undefined
      )}
      style={{
        padding: isSelected ? "13px 15px" : "14px 16px",
        background: isHovered
          ? "var(--overlay-faint)"
          : "var(--overlay-faint)",
        border: isSelected
          ? "2px solid var(--accent-primary)"
          : isHighlighted
            ? `1px solid ${withAlpha("var(--status-warning)", 40)}`
            : "1px solid var(--overlay-faint)",
        boxShadow: isHovered
          ? "var(--shadow-md)"
          : "var(--shadow-xs)",

      }}
      onMouseEnter={() => setIsHovered(true)}
      onMouseLeave={() => setIsHovered(false)}
      onClick={onViewDetail ? handleCardClick : undefined}
    >
      <div className="flex items-start gap-3 pl-2">
        {/* Content */}
        <div className="flex-1 min-w-0">
          <div className="flex items-start justify-between gap-2">
            <h3
              className="text-[0.8125rem] font-medium leading-snug tracking-[-0.01em]"
              style={{ color: "var(--text-primary)" }}
            >
              {proposal.title}
            </h3>

            {/* Actions - hidden in read-only mode */}
            {!isReadOnly && (
              <div
                className={cn(
                  "flex items-center gap-1 transition-opacity duration-150",
                  isHovered ? "opacity-100" : "opacity-0"
                )}
              >
                <TooltipProvider>
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <Button
                        variant="ghost"
                        size="icon"
                        className="h-7 w-7 rounded-lg transition-colors duration-150"
                        style={{ background: "transparent" }}
                        onMouseEnter={(e) => { e.currentTarget.style.background = "var(--overlay-weak)"; }}
                        onMouseLeave={(e) => { e.currentTarget.style.background = "transparent"; }}
                        disabled={agentGate.gated}
                        onClick={(e) => { e.stopPropagation(); if (agentGate.gated) return; onEdit(proposal.id); }}
                      >
                        <FileEdit className="w-3.5 h-3.5" style={{ color: "var(--text-muted)" }} />
                      </Button>
                    </TooltipTrigger>
                    <TooltipContent>{agentGate.reason ?? "Edit"}</TooltipContent>
                  </Tooltip>
                </TooltipProvider>
                {onDelete !== undefined && (
                  <TooltipProvider>
                    <Tooltip>
                      <TooltipTrigger asChild>
                        <Button
                          variant="ghost"
                          size="icon"
                          className="h-7 w-7 rounded-lg transition-colors duration-150"
                          style={{ background: "transparent" }}
                          onMouseEnter={(e) => { e.currentTarget.style.background = "var(--status-error-muted)"; }}
                          onMouseLeave={(e) => { e.currentTarget.style.background = "transparent"; }}
                          onClick={(e) => { e.stopPropagation(); onDelete(proposal.id); }}
                        >
                          <Trash2 className="w-3.5 h-3.5" style={{ color: "var(--status-error)" }} />
                        </Button>
                      </TooltipTrigger>
                      <TooltipContent>Delete</TooltipContent>
                    </Tooltip>
                  </TooltipProvider>
                )}
              </div>
            )}
          </div>

          <p
            className="text-[0.75rem] mt-1.5 line-clamp-2 leading-relaxed"
            style={{ color: "var(--text-secondary)" }}
          >
            {proposal.description
              ? buildProposalPreview(proposal.description)
              : "No description"}
          </p>

          {/* Tags */}
          <div className="flex flex-wrap items-center gap-2 mt-3">
            <TooltipProvider>
              {isOnCriticalPath ? (
                <Tooltip>
                  <TooltipTrigger asChild>
                    <span
                      className="px-2 py-0.5 rounded-md text-[0.625rem] font-semibold uppercase tracking-wider cursor-default"
                      style={{
                        background: priorityColor.bg,
                        border: `1px solid ${priorityColor.border}`,
                        color: priorityColor.text,
                      }}
                    >
                      {config.label}
                    </span>
                  </TooltipTrigger>
                  <TooltipContent>On critical path</TooltipContent>
                </Tooltip>
              ) : (
                <span
                  className="px-2 py-0.5 rounded-md text-[0.625rem] font-semibold uppercase tracking-wider"
                  style={{
                    background: priorityColor.bg,
                    border: `1px solid ${priorityColor.border}`,
                    color: priorityColor.text,
                  }}
                >
                  {config.label}
                </span>
              )}
            </TooltipProvider>

            <span
              className="px-2 py-0.5 rounded-md text-[0.625rem] font-medium"
              style={{
                background: "var(--overlay-faint)",
                border: "1px solid var(--overlay-weak)",
                color: "var(--text-muted)",
              }}
            >
              {proposal.category}
            </span>

            {proposal.userModified && (
              <span
                className="px-2 py-0.5 rounded-md text-[0.625rem] font-medium italic"
                style={{
                  background: "var(--status-info-muted)",
                  border: "1px solid var(--status-info-border)",
                  color: "var(--status-info)",
                }}
              >
                Modified
              </span>
            )}

            {(blocksCount !== undefined && blocksCount > 0) && (
              <TooltipProvider>
                <Tooltip>
                  <TooltipTrigger asChild>
                    <span
                      data-testid="blocks-count"
                      className="px-1.5 py-0.5 rounded text-[0.625rem] font-semibold cursor-default"
                      style={{ color: "var(--accent-primary)" }}
                    >
                      →{blocksCount}
                    </span>
                  </TooltipTrigger>
                  <TooltipContent>Blocks {blocksCount} proposal{blocksCount !== 1 ? "s" : ""}</TooltipContent>
                </Tooltip>
              </TooltipProvider>
            )}
          </div>

          {/* Dependencies */}
          {hasDependencies && (
            <div className="mt-3">
              <TooltipProvider>
                <Tooltip>
                  <TooltipTrigger asChild>
                    <button
                      data-testid="depends-on-inline"
                      onClick={(e) => { e.stopPropagation(); setIsDepsExpanded(!isDepsExpanded); }}
                      className="flex items-center gap-1.5 text-[0.6875rem] transition-colors duration-150"
                      style={{ color: "var(--text-muted)" }}
                      onMouseEnter={(e) => { e.currentTarget.style.color = "var(--text-secondary)"; }}
                      onMouseLeave={(e) => { e.currentTarget.style.color = "var(--text-muted)"; }}
                    >
                      <span data-testid="expand-dependencies" className="flex items-center">
                        {isDepsExpanded ? (
                          <ChevronDown className="w-3.5 h-3.5" />
                        ) : (
                          <ChevronRight className="w-3.5 h-3.5" />
                        )}
                      </span>
                      <span>← {inlineText}</span>
                      {overflowCount > 0 && (
                        <span style={{ opacity: 0.6 }}>+{overflowCount} more</span>
                      )}
                    </button>
                  </TooltipTrigger>
                  <TooltipContent className="max-w-xs">
                    <div className="space-y-1.5 text-[0.75rem]">
                      <div className="font-medium">
                        Depends on {dependsOnDetails!.length} proposal{dependsOnDetails!.length !== 1 ? "s" : ""}:
                      </div>
                      {dependsOnDetails!.map((dep) => (
                        <div key={dep.proposalId} style={{ color: "var(--text-muted)" }}>
                          • {dep.title}{dep.reason && `: ${dep.reason}`}
                        </div>
                      ))}
                    </div>
                  </TooltipContent>
                </Tooltip>
              </TooltipProvider>

              {isDepsExpanded && (
                <div
                  data-testid="dependencies-expanded"
                  className="mt-2 pl-4 space-y-1.5"
                  style={{
                    borderLeft: "2px solid var(--overlay-faint)",
                  }}
                >
                  {dependsOnDetails!.map((dep) => (
                    <div key={dep.proposalId} className="text-[0.6875rem]">
                      <div style={{ color: "var(--text-secondary)" }} className="font-medium">
                        {dep.title}
                      </div>
                      {dep.reason && (
                        <div style={{ color: "var(--text-muted)" }} className="italic">
                          {dep.reason}
                        </div>
                      )}
                    </div>
                  ))}
                </div>
              )}
            </div>
          )}

          {showHistoricalPlanLink && (
            <button
              data-testid="view-historical-plan"
              onClick={(e) => { e.stopPropagation(); handleViewHistoricalPlan(); }}
              className="mt-3 text-[0.75rem] flex items-center gap-1.5 transition-colors duration-150"
              style={{ color: "var(--accent-primary)" }}
              onMouseEnter={(e) => { e.currentTarget.style.color = "var(--accent-primary)"; }}
              onMouseLeave={(e) => { e.currentTarget.style.color = "var(--accent-primary)"; }}
            >
              <Eye className="w-3.5 h-3.5" />
              View plan as of proposal creation (v{proposal.planVersionAtCreation})
            </button>
          )}

          {/* View Task link - shown when proposal has been converted to task */}
          {proposal.createdTaskId && onNavigateToTask && (
            <button
              data-testid="view-task-link"
              onClick={(e) => { e.stopPropagation(); onNavigateToTask(proposal.createdTaskId!); }}
              className="mt-3 text-[0.75rem] flex items-center gap-1.5 transition-colors duration-150"
              style={{ color: "var(--accent-primary)" }}
              onMouseEnter={(e) => { e.currentTarget.style.color = "var(--accent-primary)"; }}
              onMouseLeave={(e) => { e.currentTarget.style.color = "var(--accent-primary)"; }}
            >
              <ExternalLink className="w-3.5 h-3.5" />
              View Task →
            </button>
          )}
        </div>
      </div>
    </div>
  );
});
