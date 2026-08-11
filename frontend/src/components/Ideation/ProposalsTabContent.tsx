import { useRef, useState, useCallback, useEffect, useLayoutEffect, useMemo } from "react";
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
import { useProposalStore } from "@/stores/proposalStore";
import { useIdeationStore } from "@/stores/ideationStore";
import type { IdeationSession, TaskProposal } from "@/types/ideation";
import type { DependencyGraphResponse } from "@/api/ideation.types";
import { ProposalsEmptyState } from "./ProposalsEmptyState";
import { ProposalsToolbar } from "./ProposalsToolbar";
import { TieredProposalList } from "./TieredProposalList";
import { ProactiveSyncNotificationBanner } from "./ProactiveSyncNotification";
import type { ProposalDetailEnrichment } from "./ProposalDetailSheet";

// ============================================================================
// Types
// ============================================================================

interface ProposalsTabContentProps {
  session: IdeationSession;
  proposals: TaskProposal[];
  dependencyGraph: DependencyGraphResponse | null | undefined;
  criticalPathSet: Set<string>;
  /** Highlighted proposal IDs (e.g., from sync review) — from useIdeationHandlers in parent */
  highlightedIds: Set<string>;
  isReadOnly: boolean;
  onEditProposal: (proposalId: string) => void;
  onNavigateToTask: (taskId: string) => void;
  onViewHistoricalPlan: (artifactId: string, version: number) => void;
  onViewProposal?: (proposalId: string, enrichment: ProposalDetailEnrichment) => void;
  selectedProposalId?: string | null;
  onImportPlan: () => void;
  onClearAll: () => void;
  onAcceptPlan: () => void;
  onReviewSync: () => void;
  onUndoSync: () => void;
  onDismissSync: () => void;
  onDeleteProposal?: (proposalId: string) => void;
  hideToolbar?: boolean;
}

// ============================================================================
// Component
// ============================================================================

export function ProposalsTabContent({
  session,
  proposals,
  dependencyGraph,
  criticalPathSet,
  highlightedIds,
  isReadOnly,
  onEditProposal,
  onNavigateToTask,
  onViewHistoricalPlan,
  onViewProposal,
  selectedProposalId,
  onImportPlan,
  onClearAll,
  onAcceptPlan,
  onReviewSync,
  onUndoSync,
  onDismissSync,
  onDeleteProposal,
  hideToolbar = false,
}: ProposalsTabContentProps) {
  const proposalsScrollRef = useRef<HTMLDivElement>(null);
  const [recentlyUpdatedProposalId, setRecentlyUpdatedProposalId] = useState<string | null>(null);
  const [deletingProposalId, setDeletingProposalId] = useState<string | null>(null);

  const handleDeleteRequest = useCallback((proposalId: string) => {
    setDeletingProposalId(proposalId);
  }, []);

  const handleDeleteConfirm = useCallback(() => {
    if (deletingProposalId && onDeleteProposal) {
      onDeleteProposal(deletingProposalId);
    }
    setDeletingProposalId(null);
  }, [deletingProposalId, onDeleteProposal]);

  const handleDeleteCancel = useCallback(() => {
    setDeletingProposalId(null);
  }, []);

  // Track scroll state across session switches and proposal changes
  const lastScrollSessionIdRef = useRef<string | null>(null);
  const lastScrollProposalAddedAtRef = useRef<number | null>(null);
  const lastScrollProposalUpdatedAtRef = useRef<number | null>(null);

  const planArtifact = useIdeationStore((state) => state.planArtifact);
  const syncNotification = useIdeationStore((state) => state.syncNotification);

  const lastProposalAddedAt = useProposalStore((state) => state.lastProposalAddedAt[session.id] ?? null);
  const lastProposalUpdatedAt = useProposalStore((state) => state.lastProposalUpdatedAt[session.id] ?? null);
  const lastUpdatedProposalId = useProposalStore((state) => state.lastUpdatedProposalId[session.id] ?? null);

  // Auto-scroll to bottom when a new proposal is added
  useLayoutEffect(() => {
    const currentSessionId = session.id;
    if (lastScrollSessionIdRef.current !== currentSessionId) {
      lastScrollSessionIdRef.current = currentSessionId;
      lastScrollProposalAddedAtRef.current = null;
      lastScrollProposalUpdatedAtRef.current = null;
      proposalsScrollRef.current?.scrollTo?.({ top: 0, behavior: "auto" });
      return;
    }

    if (!lastProposalAddedAt || lastProposalAddedAt === lastScrollProposalAddedAtRef.current) {
      return;
    }

    if (proposalsScrollRef.current?.scrollTo) {
      proposalsScrollRef.current.scrollTo({
        top: proposalsScrollRef.current.scrollHeight,
        behavior: "smooth",
      });
    }
    lastScrollProposalAddedAtRef.current = lastProposalAddedAt;
  }, [lastProposalAddedAt, session.id]);

  // Auto-scroll to updated proposal
  useLayoutEffect(() => {
    if (!lastProposalUpdatedAt || lastProposalUpdatedAt === lastScrollProposalUpdatedAtRef.current) {
      return;
    }

    if (!lastUpdatedProposalId) {
      return;
    }

    if (!proposals.some((p) => p.id === lastUpdatedProposalId)) {
      return;
    }

    if (proposalsScrollRef.current) {
      const target = proposalsScrollRef.current.querySelector(
        `[data-testid="proposal-card-${lastUpdatedProposalId}"]`
      );
      if (target instanceof HTMLElement) {
        target.scrollIntoView({ behavior: "smooth", block: "center" });
      }
    }

    setRecentlyUpdatedProposalId(lastUpdatedProposalId);
    lastScrollProposalUpdatedAtRef.current = lastProposalUpdatedAt;
  }, [lastProposalUpdatedAt, lastUpdatedProposalId, proposals]);

  // Clear recently-updated highlight after 2.4 s
  useEffect(() => {
    if (!recentlyUpdatedProposalId) return;
    const timeout = setTimeout(() => setRecentlyUpdatedProposalId(null), 2400);
    return () => clearTimeout(timeout);
  }, [recentlyUpdatedProposalId]);

  // Merge parent-highlighted IDs with the locally-updated one
  const highlightedProposalIdsWithUpdates = useMemo(() => {
    if (!recentlyUpdatedProposalId) return highlightedIds;
    const merged = new Set(highlightedIds);
    merged.add(recentlyUpdatedProposalId);
    return merged;
  }, [highlightedIds, recentlyUpdatedProposalId]);

  return (
    <div className="flex flex-col flex-1 min-h-0 overflow-hidden">
      {/* Proposals toolbar — only when there are proposals */}
      {!hideToolbar && proposals.length > 0 && (
        <ProposalsToolbar
          proposals={proposals}
          graph={dependencyGraph}
          isReadOnly={isReadOnly}
          onClearAll={onClearAll}
          onAcceptPlan={onAcceptPlan}
          isPendingAcceptance={session.acceptanceStatus === "pending"}
        />
      )}

      <div ref={proposalsScrollRef} className="flex-1 overflow-y-auto p-4">
        {/* Sync notification — proposals may need update after plan change */}
        {syncNotification && (
          <ProactiveSyncNotificationBanner
            notification={syncNotification}
            onDismiss={onDismissSync}
            onReview={onReviewSync}
            onUndo={onUndoSync}
          />
        )}

        {proposals.length === 0 && (
          <ProposalsEmptyState onBrowse={onImportPlan} />
        )}

        {proposals.length > 0 && (
          <TieredProposalList
            proposals={proposals}
            dependencyGraph={dependencyGraph}
            highlightedIds={highlightedProposalIdsWithUpdates}
            criticalPathIds={criticalPathSet}
            onEdit={onEditProposal}
            {...(planArtifact?.metadata.version !== undefined && {
              currentPlanVersion: planArtifact.metadata.version,
            })}
            {...(isReadOnly && { isReadOnly })}
            onNavigateToTask={onNavigateToTask}
            onViewHistoricalPlan={onViewHistoricalPlan}
            {...(onViewProposal !== undefined && { onViewDetail: onViewProposal })}
            {...(selectedProposalId != null && { selectedProposalId })}
            {...(!isReadOnly && onDeleteProposal !== undefined && { onDelete: handleDeleteRequest })}
          />
        )}
      </div>

      <AlertDialog open={deletingProposalId !== null} onOpenChange={(open) => { if (!open) handleDeleteCancel(); }}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete proposal?</AlertDialogTitle>
            <AlertDialogDescription>
              This will permanently delete the proposal. This action cannot be undone.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel onClick={handleDeleteCancel}>Cancel</AlertDialogCancel>
            <AlertDialogAction onClick={handleDeleteConfirm}>Delete</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
