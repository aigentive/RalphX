/**
 * TieredProposalList - macOS Tahoe styled proposal tier system
 *
 * Design: Clean vertical flow with subtle connectors,
 * warm orange critical path highlighting, and refined animations.
 */

import React, { useMemo } from "react";
import type { TaskProposal } from "@/types/ideation";
import type { DependencyGraphResponse } from "@/api/ideation.types";
import { ProposalCard, type DependencyDetail } from "./ProposalCard";
import type { ProposalDetailEnrichment } from "./ProposalDetailSheet";
import { ProposalTierGroup } from "./ProposalTierGroup";
import { useDependencyTiers, getDependencyReason } from "@/hooks/useDependencyGraph";
import { withAlpha } from "@/lib/theme-colors";

// ============================================================================
// Tier Connector Component
// ============================================================================

interface TierConnectorProps {
  /** Whether the connector is on the critical path */
  isOnCriticalPath: boolean;
}

/**
 * Refined connector between tier groups with subtle animation
 */
const TierConnector = React.memo(function TierConnector({
  isOnCriticalPath,
}: TierConnectorProps) {
  return (
    <div
      data-testid="tier-connector"
      className="flex justify-center py-2"
      aria-hidden="true"
    >
      <div className="relative">
        {/* Glow effect for critical path */}
        {isOnCriticalPath && (
          <div
            className="absolute inset-0 blur-md"
            style={{
              background: `radial-gradient(circle, ${withAlpha("var(--accent-primary)", 30)} 0%, transparent 70%)`,
            }}
          />
        )}
        <svg
          width="24"
          height="24"
          viewBox="0 0 24 24"
          fill="none"
          className="relative"
        >
          {/* Vertical connector line */}
          <line
            x1="12"
            y1="0"
            x2="12"
            y2="16"
            stroke={isOnCriticalPath ? "var(--accent-primary)" : "var(--border-subtle)"}
            strokeWidth={isOnCriticalPath ? "2" : "1"}
            strokeDasharray={isOnCriticalPath ? "none" : "3 4"}
            strokeLinecap="round"
            style={{
              filter: isOnCriticalPath ? `drop-shadow(0 0 4px ${withAlpha("var(--accent-primary)", 50)})` : "none",
            }}
          />
          {/* Downward chevron */}
          <path
            d="M8 14L12 20L16 14"
            stroke={isOnCriticalPath ? "var(--accent-primary)" : "var(--overlay-moderate)"}
            strokeWidth={isOnCriticalPath ? "2" : "1"}
            strokeLinecap="round"
            strokeLinejoin="round"
            fill="none"
            style={{
              filter: isOnCriticalPath ? `drop-shadow(0 0 4px ${withAlpha("var(--accent-primary)", 50)})` : "none",
            }}
          />
        </svg>
      </div>
    </div>
  );
});

// ============================================================================
// Types
// ============================================================================

export interface TieredProposalListProps {
  /** List of proposals to display */
  proposals: TaskProposal[];
  /** Dependency graph for tier computation and dependency details */
  dependencyGraph: DependencyGraphResponse | null | undefined;
  /** Currently selected/highlighted proposal IDs */
  highlightedIds: Set<string>;
  /** IDs of proposals on the critical path */
  criticalPathIds: Set<string>;
  /** Current plan version for historical link display */
  currentPlanVersion?: number;
  /** Callback when a proposal is selected */
  /** Callback when a proposal is edited */
  onEdit: (proposalId: string) => void;
  /** Callback to view historical plan */
  onViewHistoricalPlan?: (artifactId: string, version: number) => void;
  /** Whether the plan is in read-only mode (accepted/archived status) */
  isReadOnly?: boolean;
  /** Callback to navigate to a created task in kanban */
  onNavigateToTask?: (taskId: string) => void;
  /** Callback when card is clicked to open detail sheet */
  onViewDetail?: (proposalId: string, enrichment: ProposalDetailEnrichment) => void;
  /** ID of the currently selected proposal (for highlight state) */
  selectedProposalId?: string | null;
  /** Callback to delete a proposal (passes proposalId for confirmation in parent) */
  onDelete?: (proposalId: string) => void;
}

// ============================================================================
// Helpers
// ============================================================================

/**
 * Build dependency details for a proposal from the graph
 */
function buildDependencyDetails(
  proposalId: string,
  proposals: TaskProposal[],
  dependencyGraph: DependencyGraphResponse | null | undefined
): DependencyDetail[] {
  if (!dependencyGraph) return [];

  const details: DependencyDetail[] = [];

  for (const edge of dependencyGraph.edges) {
    // Dependency-detail payloads store the dependent in `from` and its
    // prerequisite in `to`; preserve that persisted presentation contract.
    if (edge.from === proposalId) {
      const targetProposal = proposals.find(p => p.id === edge.to);
      if (targetProposal) {
        const reason = getDependencyReason(dependencyGraph, edge.from, edge.to);
        const detail: DependencyDetail = {
          proposalId: edge.to,
          title: targetProposal.title,
        };
        if (reason !== undefined) {
          detail.reason = reason;
        }
        details.push(detail);
      }
    }
  }

  return details;
}

/**
 * Compute blocks count for a proposal (how many proposals depend on this one)
 */
function computeBlocksCount(
  proposalId: string,
  dependencyGraph: DependencyGraphResponse | null | undefined
): number {
  if (!dependencyGraph) return 0;

  // Count edges where edge.to === proposalId (this proposal is depended upon)
  return dependencyGraph.edges.filter(edge => edge.to === proposalId).length;
}

// ============================================================================
// Component
// ============================================================================

export const TieredProposalList = React.memo(function TieredProposalList({
  proposals,
  dependencyGraph,
  highlightedIds,
  criticalPathIds,
  currentPlanVersion,
  onEdit,
  onViewHistoricalPlan,
  isReadOnly,
  onNavigateToTask,
  onViewDetail,
  selectedProposalId,
  onDelete,
}: TieredProposalListProps) {
  // Compute tier assignments from dependency graph
  const { tierMap, maxTier } = useDependencyTiers(dependencyGraph);

  // Group proposals by tier, sorted within each tier by sortOrder
  const proposalsByTier = useMemo(() => {
    const tiers = new Map<number, TaskProposal[]>();

    // Initialize tier groups
    for (let i = 0; i <= maxTier; i++) {
      tiers.set(i, []);
    }

    // Assign proposals to tiers
    for (const proposal of proposals) {
      const tier = tierMap.get(proposal.id) ?? 0;
      const tierProposals = tiers.get(tier);
      if (tierProposals) {
        tierProposals.push(proposal);
      } else {
        tiers.set(tier, [proposal]);
      }
    }

    // Sort proposals within each tier by sortOrder
    for (const [tier, tierProposals] of tiers) {
      tiers.set(tier, tierProposals.sort((a, b) => a.sortOrder - b.sortOrder));
    }

    return tiers;
  }, [proposals, tierMap, maxTier]);

  // Get ordered tier numbers (0, 1, 2, ...) - only non-empty tiers
  const tierNumbers = useMemo(() => {
    return Array.from(proposalsByTier.keys())
      .filter(tier => (proposalsByTier.get(tier) ?? []).length > 0)
      .sort((a, b) => a - b);
  }, [proposalsByTier]);

  // Compute which tier transitions have critical path proposals on both sides
  // A connector is "critical" if there's a critical path proposal in both adjacent tiers
  const criticalConnectors = useMemo(() => {
    const critical = new Set<number>(); // Set of tier numbers where connector TO that tier is critical

    for (let i = 1; i < tierNumbers.length; i++) {
      const prevTier = tierNumbers[i - 1]!; // Guaranteed by loop bounds
      const currTier = tierNumbers[i]!; // Guaranteed by loop bounds

      const prevTierProposals = proposalsByTier.get(prevTier) ?? [];
      const currTierProposals = proposalsByTier.get(currTier) ?? [];

      // Check if any proposal in prev tier is on critical path
      const prevHasCritical = prevTierProposals.some(p => criticalPathIds.has(p.id));
      // Check if any proposal in curr tier is on critical path
      const currHasCritical = currTierProposals.some(p => criticalPathIds.has(p.id));

      // Connector is critical if both tiers have critical path proposals
      if (prevHasCritical && currHasCritical) {
        critical.add(currTier);
      }
    }

    return critical;
  }, [tierNumbers, proposalsByTier, criticalPathIds]);

  // If no proposals, return null (parent handles empty state)
  if (proposals.length === 0) {
    return null;
  }

  const firstTier = tierNumbers[0];

  return (
    <div data-testid="tiered-proposal-list" className="space-y-1">
      {tierNumbers.map((tier, tierIndex) => {
        const tierProposals = proposalsByTier.get(tier) ?? [];

        // Render connector before tier (except for the first tier)
        const showConnector = tierIndex > 0;
        const connectorIsCritical = criticalConnectors.has(tier);

        return (
          <React.Fragment key={tier}>
            {showConnector && (
              <TierConnector isOnCriticalPath={connectorIsCritical} />
            )}
            <ProposalTierGroup
              tier={tier}
              proposalCount={tierProposals.length}
              {...(firstTier !== undefined && tier === firstTier && { defaultCollapsed: false })}
            >
              <div className="space-y-2">
                {tierProposals.map((proposal, index) => {
                  const dependsOnDetails = buildDependencyDetails(
                    proposal.id,
                    proposals,
                    dependencyGraph
                  );
                  const blocksCount = computeBlocksCount(proposal.id, dependencyGraph);
                  const isOnCriticalPath = criticalPathIds.has(proposal.id);
                  const isHighlighted = highlightedIds.has(proposal.id);

                  // Build optional props conditionally for exactOptionalPropertyTypes
                  const optionalProps: {
                    dependsOnCount?: number;
                    dependsOnDetails?: DependencyDetail[];
                    blocksCount?: number;
                    isOnCriticalPath?: boolean;
                    isHighlighted?: boolean;
                    currentPlanVersion?: number;
                    onViewHistoricalPlan?: (artifactId: string, version: number) => void;
                    isReadOnly?: boolean;
                    onNavigateToTask?: (taskId: string) => void;
                    onViewDetail?: (proposalId: string, enrichment: ProposalDetailEnrichment) => void;
                    isSelected?: boolean;
                    onDelete?: (proposalId: string) => void;
                  } = {};

                  if (dependsOnDetails.length > 0) {
                    optionalProps.dependsOnCount = dependsOnDetails.length;
                    optionalProps.dependsOnDetails = dependsOnDetails;
                  }
                  if (blocksCount > 0) {
                    optionalProps.blocksCount = blocksCount;
                  }
                  if (isOnCriticalPath) {
                    optionalProps.isOnCriticalPath = isOnCriticalPath;
                  }
                  if (isHighlighted) {
                    optionalProps.isHighlighted = isHighlighted;
                  }
                  if (currentPlanVersion !== undefined) {
                    optionalProps.currentPlanVersion = currentPlanVersion;
                  }
                  if (onViewHistoricalPlan !== undefined) {
                    optionalProps.onViewHistoricalPlan = onViewHistoricalPlan;
                  }
                  if (isReadOnly) {
                    optionalProps.isReadOnly = isReadOnly;
                  }
                  if (onNavigateToTask !== undefined) {
                    optionalProps.onNavigateToTask = onNavigateToTask;
                  }
                  if (onViewDetail !== undefined) {
                    optionalProps.onViewDetail = onViewDetail;
                  }
                  if (selectedProposalId === proposal.id) {
                    optionalProps.isSelected = true;
                  }
                  if (onDelete !== undefined) {
                    optionalProps.onDelete = onDelete;
                  }

                  return (
                    <div
                      key={proposal.id}
                      className="animate-in fade-in slide-in-from-bottom-2"
                      style={{
                        animationDelay: `${index * 40}ms`,
                        animationDuration: "300ms",
                        animationFillMode: "both",
                      }}
                    >
                      <ProposalCard
                        proposal={proposal}
                        onEdit={onEdit}
                        {...optionalProps}
                      />
                    </div>
                  );
                })}
              </div>
            </ProposalTierGroup>
          </React.Fragment>
        );
      })}
    </div>
  );
});

export default TieredProposalList;
