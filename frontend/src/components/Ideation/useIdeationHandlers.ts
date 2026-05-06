/**
 * useIdeationHandlers - Event handlers for IdeationView
 */

import { useCallback, useRef, useState } from "react";
import { PRIORITY_VALUES, type TaskProposal, type IdeationSession } from "@/types/ideation";
import { useIdeationStore, type ProactiveSyncNotification } from "@/stores/ideationStore";
import { ideationApi } from "@/api/ideation";
import { backendApiUrl } from "@/api/backend";

export function useIdeationHandlers(
  session: IdeationSession | null,
  proposals: TaskProposal[],
  onRemoveProposal: (proposalId: string) => void,
  onReorderProposals: (proposalIds: string[]) => void,
  onArchiveSession: (sessionId: string) => void,
  fetchPlanArtifact: (artifactId: string) => Promise<void>,
  dismissSyncNotification: () => void,
  syncNotification: ProactiveSyncNotification | null
) {
  const [highlightedProposalIds, setHighlightedProposalIds] = useState<Set<string>>(new Set());
  const [isPlanExpanded, setIsPlanExpanded] = useState(false);
  const [importStatus, setImportStatus] = useState<{ type: "success" | "error"; message: string } | null>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const isImportingRef = useRef<boolean>(false);
  const importTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const updateSession = useIdeationStore((state) => state.updateSession);

  const handleArchive = useCallback(() => {
    if (session) onArchiveSession(session.id);
  }, [session, onArchiveSession]);

  const handleSortByPriority = useCallback(() => {
    // Sort by suggestedPriority (critical > high > medium > low)
    // PRIORITY_VALUES is ordered by importance, so lower index = higher priority
    const sorted = [...proposals].sort((a, b) => {
      const aIndex = PRIORITY_VALUES.indexOf(a.suggestedPriority);
      const bIndex = PRIORITY_VALUES.indexOf(b.suggestedPriority);
      return aIndex - bIndex;
    });
    onReorderProposals(sorted.map((p) => p.id));
  }, [proposals, onReorderProposals]);

  const handleClearAll = useCallback(() => {
    proposals.forEach((p) => onRemoveProposal(p.id));
  }, [proposals, onRemoveProposal]);

  const handleReviewSync = useCallback(() => {
    if (syncNotification) {
      setHighlightedProposalIds(new Set(syncNotification.proposalIds));
      setTimeout(() => setHighlightedProposalIds(new Set()), 5000);
    }
  }, [syncNotification]);

  const handleUndoSync = useCallback(() => {
    if (!syncNotification) return;
    dismissSyncNotification();
    setHighlightedProposalIds(new Set());
  }, [syncNotification, dismissSyncNotification]);

  const handleDismissSync = useCallback(() => {
    dismissSyncNotification();
    setHighlightedProposalIds(new Set());
  }, [dismissSyncNotification]);

  const handleImportPlan = useCallback(() => fileInputRef.current?.click(), []);

  const importPlanContent = useCallback(async (title: string, content: string) => {
    if (!session) return;
    if (isImportingRef.current) return;

    isImportingRef.current = true;
    try {
      const apiResponse = await fetch(backendApiUrl("create_plan_artifact"), {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ session_id: session.id, title, content }),
      });

      if (!apiResponse.ok) throw new Error("Failed to import plan");

      const data = await apiResponse.json();
      if (data.id) {
        await fetchPlanArtifact(data.id);
        // Update session in store so planArtifactId persists across navigation
        updateSession(session.id, { planArtifactId: data.id });
        if (importTimeoutRef.current !== null) clearTimeout(importTimeoutRef.current);
        setImportStatus({ type: "success", message: `Plan "${title}" imported successfully` });
        importTimeoutRef.current = setTimeout(() => setImportStatus(null), 5000);

        // Spawn session namer agent with plan content as context (fire-and-forget)
        const contentPreview = content.slice(0, 500);
        const context = `Plan imported: "${title}"\n\nContent preview:\n${contentPreview}`;
        ideationApi.sessions.spawnSessionNamer(session.id, context).catch((err) => {
          console.error("Failed to spawn session namer:", err);
        });
      }
    } catch (error) {
      console.error("Plan import error:", error);
      if (importTimeoutRef.current !== null) clearTimeout(importTimeoutRef.current);
      setImportStatus({ type: "error", message: error instanceof Error ? error.message : "Failed to import plan" });
      importTimeoutRef.current = setTimeout(() => setImportStatus(null), 5000);
    } finally {
      isImportingRef.current = false;
    }
  }, [session, fetchPlanArtifact, updateSession]);

  const handleFileSelected = useCallback(async (event: React.ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    if (!file) return;
    try {
      const content = await file.text();
      const title = file.name.replace(/\.md$/, "").replace(/_/g, " ");
      await importPlanContent(title, content);
    } finally {
      if (fileInputRef.current) fileInputRef.current.value = "";
    }
  }, [importPlanContent]);

  // Handler for drag-and-drop file import (used with useFileDrop)
  const handleFileDrop = useCallback(async (file: File, content: string) => {
    const title = file.name.replace(/\.md$/, "").replace(/_/g, " ");
    await importPlanContent(title, content);
  }, [importPlanContent]);

  return {
    highlightedProposalIds,
    isPlanExpanded,
    setIsPlanExpanded,
    importStatus,
    setImportStatus,
    fileInputRef,
    handleArchive,
    handleSortByPriority,
    handleClearAll,
    handleReviewSync,
    handleUndoSync,
    handleDismissSync,
    handleImportPlan,
    handleFileSelected,
    handleFileDrop,
  };
}
