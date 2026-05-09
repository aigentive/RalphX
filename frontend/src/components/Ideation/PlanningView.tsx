/**
 * PlanningView - Premium Planning Interface
 *
 * Design: macOS Tahoe Liquid Glass
 * - Flat translucent surfaces with backdrop-blur
 * - Subtle borders and single shadows
 * - Warm ambient orange glow in backgrounds
 * - Clean, minimal aesthetic
 */

import { useState, useCallback, useRef, useEffect, useMemo } from "react";
import {
  MessageSquare,
  Archive,
  Loader2,
  RotateCcw,
  RefreshCw,
  ArrowLeft,
  CheckCircle,
  AlertTriangle,
  ShieldCheck,
  Menu,
} from "lucide-react";
import { useEventBus } from "@/providers/EventProvider";
import { toast } from "sonner";
import type {
  IdeationSession,
  TaskProposal,
  VerificationStatus,
} from "@/types/ideation";
import type { ApplyProposalsInput, ApplyProposalsResultResponse } from "@/api/ideation.types";
import { Button } from "@/components/ui/button";
import { withAlpha } from "@/lib/theme-colors";
import { ResizeHandle, CHAT_PANEL_DEFAULT_WIDTH, CHAT_PANEL_MIN_WIDTH } from "@/components/ui/ResizeHandle";
import { getTeamArtifacts } from "@/api/team";
import type { TeamArtifactSummary } from "@/api/team";
import { useTeamStore } from "@/stores/teamStore";
import { useUiStore } from "@/stores/uiStore";
import { useIdeationStore } from "@/stores/ideationStore";
import { useProposalStore } from "@/stores/proposalStore";
import { usePlanStore } from "@/stores/planStore";
import { useProjectStore, selectActiveProject } from "@/stores/projectStore";
import { AcceptModal } from "./AcceptModal";
import { IntegratedChatPanel } from "@/components/Chat/IntegratedChatPanel";
import { ConversationEmptyState, WaitingForCapacityState } from "./EmptyStates";
import { useChildSessionStatus } from "@/hooks/useChildSessionStatus";
import { animationStyles } from "./PlanningView.constants";
import { PlanBrowser } from "./PlanBrowser";
import { StartSessionPanel } from "./StartSessionPanel";
import type { TeamMetadata } from "./PlanDisplay";
import type { ProposalDetailEnrichment } from "./ProposalDetailSheet";
import { useIdeationHandlers } from "./useIdeationHandlers";
import { useFileDrop } from "@/hooks/useFileDrop";
import { useDependencyGraph } from "@/hooks/useDependencyGraph";
import { DropZoneOverlay } from "./DropZoneOverlay";
import { ReopenSessionDialog } from "./ReopenSessionDialog";
import type { ReopenMode } from "./ReopenSessionDialog";
import { useReopenSession, useResetAndReaccept, useIdeationSessions } from "@/hooks/useIdeation";
import { usePlanBrowserLayout } from "@/hooks/usePlanBrowserLayout";
import { usePersistentSidebarResize } from "@/hooks/usePersistentSidebarResize";
import { ideationApi } from "@/api/ideation";
import { useQuery } from "@tanstack/react-query";
import { planBranchApi } from "@/api/plan-branch";
import { useTasks } from "@/hooks/useTasks";
import { PlanTabContent } from "./PlanTabContent";
import { ProposalsTabContent } from "./ProposalsTabContent";
import { TeamResearchTabContent } from "./TeamResearchTabContent";
import { VerificationPanel } from "./VerificationPanel";

// ============================================================================
// Types
// ============================================================================

interface PlanningViewProps {
  session: IdeationSession | null;
  proposals: TaskProposal[];
  isSessionLoading?: boolean;
  onNewSession: () => void;
  onSelectSession: (sessionId: string) => void;
  onArchiveSession: (sessionId: string) => void;
  onEditProposal: (proposalId: string) => void;
  onRemoveProposal: (proposalId: string) => void;
  onReorderProposals: (proposalIds: string[]) => void;
  onApply: (options: ApplyProposalsInput) => Promise<ApplyProposalsResultResponse>;
  /** Callback to open proposal detail sheet */
  onViewProposal?: (proposalId: string, enrichment: ProposalDetailEnrichment) => void;
  /** ID of currently selected proposal (for highlight state in cards) */
  selectedProposalId?: string | null;
  /** Footer slot for execution controls — renders below left section */
  footer?: React.ReactNode;
}

// Empty States extracted to separate files

// Plan Browser extracted to PlanBrowser.tsx

// Start Session Panel extracted to StartSessionPanel.tsx

// Proposal Card extracted to ProposalCard.tsx

// Proactive Sync Notification extracted to ProactiveSyncNotification.tsx

// Proposals Toolbar extracted to ProposalsToolbar.tsx

// ============================================================================
// Analysis Banner
// ============================================================================

/** Prominent banner shown below ProposalsToolbar while dependency analysis runs. */
export function AnalysisBanner() {
  return (
    <div
      data-testid="analysis-banner"
      className="flex items-center gap-2 px-4 py-2 shrink-0"
      style={{
        background: withAlpha("var(--accent-primary)", 6),
        borderBottom: "1px solid var(--accent-border)",
      }}
    >
      <Loader2
        className="w-3.5 h-3.5 animate-spin shrink-0"
        style={{ color: "var(--accent-primary)" }}
      />
      <span className="text-[0.75rem]" style={{ color: "var(--accent-primary)" }}>
        Analyzing dependencies — accept will be available when complete
      </span>
    </div>
  );
}

// ============================================================================
// Main Component
// ============================================================================

const PLAN_BROWSER_SIDEBAR_WIDTH_STORAGE_KEY = "ralphx-plan-browser-sidebar-width";
const PLAN_BROWSER_SIDEBAR_MIN_WIDTH = 220;
const PLAN_BROWSER_SIDEBAR_MAX_WIDTH = 520;

export function PlanningView({
  session,
  proposals,
  isSessionLoading = false,
  onNewSession,
  onSelectSession,
  onArchiveSession,
  onEditProposal,
  onRemoveProposal,
  onReorderProposals,
  onApply,
  onViewProposal,
  selectedProposalId,
  footer,
}: PlanningViewProps) {
  const [chatPanelWidth, setChatPanelWidth] = useState(CHAT_PANEL_DEFAULT_WIDTH);
  const [isResizing, setIsResizing] = useState(false);
  const [isAcceptModalOpen, setIsAcceptModalOpen] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const planBrowserSidebarRef = useRef<HTMLDivElement | null>(null);

  const {
    sidebarWidth,
    isCollapsed,
    isOverlayOpen,
    toggleCollapse,
    closeOverlay,
    suppressTransition,
  } = usePlanBrowserLayout();
  const {
    handleSidebarResizeReset: handlePlanBrowserResizeReset,
    handleSidebarResizeStart: handlePlanBrowserResizeStart,
    isSidebarResizing: isPlanBrowserSidebarResizing,
    userSidebarWidth: userPlanBrowserSidebarWidth,
  } = usePersistentSidebarResize(planBrowserSidebarRef, {
    maxWidth: PLAN_BROWSER_SIDEBAR_MAX_WIDTH,
    minWidth: PLAN_BROWSER_SIDEBAR_MIN_WIDTH,
    storageKey: PLAN_BROWSER_SIDEBAR_WIDTH_STORAGE_KEY,
  });
  const effectiveSidebarWidth =
    !isCollapsed && userPlanBrowserSidebarWidth !== null && sidebarWidth > 0
      ? userPlanBrowserSidebarWidth
      : sidebarWidth;
  const showPlanBrowserResizeHandle =
    !isCollapsed && !isOverlayOpen && sidebarWidth > 0;
  const planBrowserTransition =
    suppressTransition.current || isPlanBrowserSidebarResizing
      ? "none"
      : "width 300ms ease";

  const planArtifact = useIdeationStore((state) => state.planArtifact);
  const fetchPlanArtifact = useIdeationStore((state) => state.fetchPlanArtifact);
  const showSyncNotification = useIdeationStore((state) => state.showSyncNotification);
  const dismissSyncNotification = useIdeationStore((state) => state.dismissSyncNotification);
  const syncNotification = useIdeationStore((state) => state.syncNotification);

  // Fetch dependency graph for the session
  const { data: dependencyGraph, isFetching: isDependencyUpdating } = useDependencyGraph(session?.id ?? "");

  // Build critical path set from the graph
  const criticalPathSet = useMemo(() => {
    if (!dependencyGraph) return new Set<string>();
    return new Set(dependencyGraph.criticalPath);
  }, [dependencyGraph]);

  const lastDependencyFetchRef = useRef<boolean>(false);
  const lastDependencyToastAtRef = useRef<number | null>(null);
  const lastDependencyRefreshRequestedAt = useProposalStore((state) => session?.id ? (state.lastDependencyRefreshRequestedAt[session.id] ?? null) : null);

  // Read-only mode: plans that are not active are read-only
  const isReadOnly = session?.status !== "active";

  // Reopen/Reset dialog state
  const [reopenDialogOpen, setReopenDialogOpen] = useState(false);
  const [reopenDialogMode, setReopenDialogMode] = useState<ReopenMode>("reopen");
  const reopenMutation = useReopenSession();
  const resetMutation = useResetAndReaccept();

  // Count tasks created from this session's proposals
  const sessionTaskCount = useMemo(
    () => proposals.filter((p) => p.createdTaskId != null).length,
    [proposals]
  );

  // Plan store actions
  const setActivePlan = usePlanStore((state) => state.setActivePlan);
  const clearActivePlan = usePlanStore((state) => state.clearActivePlan);
  const activePlanByProject = usePlanStore((state) => state.activePlanByProject);
  const activeProjectId = useProjectStore((state) => state.activeProjectId);

  // Sessions list for breadcrumb parent resolution
  const projectIdForSessions = activeProjectId || session?.projectId || "";
  const { data: allSessionsForBreadcrumb = [] } = useIdeationSessions(projectIdForSessions);
  const { data: projectTasks = [] } = useTasks(projectIdForSessions);

  const sourceTaskTitle = session?.sourceTaskId
    ? projectTasks.find((task) => task.id === session.sourceTaskId)?.title ?? null
    : null;

  const sourceContextLabel = (() => {
    switch (session?.sourceContextType) {
      case "task_execution":
        return "Execution follow-up";
      case "review":
        return "Review follow-up";
      case "merge":
        return "Merge follow-up";
      case "research":
        return "Research follow-up";
      default:
        return session?.sourceContextType ? "Follow-up" : null;
    }
  })();

  const canReopen = isReadOnly && (session?.status === "accepted" || session?.status === "archived");
  const canResetReaccept = session?.status === "accepted";

  const handleOpenReopenDialog = useCallback((mode: ReopenMode) => {
    setReopenDialogMode(mode);
    setReopenDialogOpen(true);
  }, []);

  const handleConfirmReopen = useCallback(() => {
    if (!session) return;
    if (reopenDialogMode === "reopen") {
      reopenMutation.mutate(session.id, {
        onSuccess: async () => {
          setReopenDialogOpen(false);
          toast.success("Session reopened");

          // Clear active plan if this session was the active plan
          if (activeProjectId) {
            const activePlanId = activePlanByProject[activeProjectId];
            if (activePlanId === session.id) {
              try {
                await clearActivePlan(activeProjectId);
              } catch (err) {
                console.error("Failed to clear active plan:", err);
                toast.error("Failed to clear active plan");
              }
            }
          }
        },
        onError: (err) => toast.error(`Failed to reopen: ${err.message}`),
      });
    } else {
      resetMutation.mutate(
        { sessionId: session.id, proposalIds: proposals.map((p) => p.id) },
        {
          onSuccess: () => {
            setReopenDialogOpen(false);
            toast.success("Session reset and re-accepted");
          },
          onError: (err) => toast.error(`Failed to reset: ${err.message}`),
        }
      );
    }
  }, [session, reopenDialogMode, reopenMutation, resetMutation, proposals, activeProjectId, activePlanByProject, clearActivePlan]);

  // Get the event bus from context (TauriEventBus or MockEventBus)
  const eventBus = useEventBus();

  // Small UX hint when dependency graph refreshes automatically
  useEffect(() => {
    if (!session?.id || proposals.length === 0) {
      lastDependencyFetchRef.current = false;
      return;
    }

    if (
      !lastDependencyRefreshRequestedAt
      || lastDependencyRefreshRequestedAt === lastDependencyToastAtRef.current
    ) {
      lastDependencyFetchRef.current = false;
      return;
    }

    if (isDependencyUpdating) {
      lastDependencyFetchRef.current = true;
      return;
    }

    if (lastDependencyFetchRef.current) {
      toast.success("Dependencies updated");
      lastDependencyFetchRef.current = false;
      lastDependencyToastAtRef.current = lastDependencyRefreshRequestedAt;
    }
  }, [isDependencyUpdating, session?.id, proposals.length, lastDependencyRefreshRequestedAt]);

  // Stable ref for fetchPlanArtifact (must stay in PlanningView — always mounted)
  const fetchPlanArtifactRef = useRef(fetchPlanArtifact);
  useEffect(() => { fetchPlanArtifactRef.current = fetchPlanArtifact; }, [fetchPlanArtifact]);

  const planArtifactId = planArtifact?.id ?? null;
  const effectivePlanId = session?.planArtifactId ?? session?.inheritedPlanArtifactId ?? null;
  useEffect(() => {
    if (!effectivePlanId) return;
    if (planArtifactId !== effectivePlanId) {
      fetchPlanArtifactRef.current(effectivePlanId);
    }
  }, [effectivePlanId, planArtifactId, session?.planArtifactId, session?.inheritedPlanArtifactId]);

  // Fetch plan branch to show preserved branch config in reopen dialog
  const { data: planBranch } = useQuery({
    queryKey: ["plan-branch", planArtifactId],
    queryFn: () => planBranchApi.getByPlan(planArtifactId!),
    enabled: planArtifactId != null,
  });

  // Fetch team artifact summaries for team-ideated sessions
  const [teamArtifacts, setTeamArtifacts] = useState<TeamArtifactSummary[]>([]);
  const artifactVersion = useTeamStore((s) => s.artifactVersion[session?.id ?? ""] ?? 0);
  useEffect(() => {
    const hasTeamMode = session?.teamMode && session.teamMode !== "solo";
    const hasArtifacts = artifactVersion > 0;
    if (!session?.id || (!hasTeamMode && !hasArtifacts)) {
      setTeamArtifacts([]);
      return;
    }

    let cancelled = false;
    getTeamArtifacts(session.id)
      .then((resp) => {
        if (cancelled) return;
        setTeamArtifacts(resp.artifacts);
      })
      .catch((err) => {
        if (cancelled) return;
        console.error("Failed to fetch team artifacts:", err);
        setTeamArtifacts([]);
      });

    return () => { cancelled = true; };
  }, [session?.id, session?.teamMode, artifactVersion]);

  // Tab state managed in Zustand (enables cross-component tab switching from notifications)
  const setActiveIdeationTab = useIdeationStore((s) => s.setActiveIdeationTab);
  const setActiveVerificationChildId = useIdeationStore((s) => s.setActiveVerificationChildId);
  const setLastVerificationChildId = useIdeationStore((s) => s.setLastVerificationChildId);
  const activeTab = useIdeationStore(
    (s) => s.activeIdeationTab[session?.id ?? ''] ?? 'plan'
  );
  const activeVerificationChildId = useIdeationStore(
    (s) => s.activeVerificationChildId[session?.id ?? ''] ?? null
  );
  const lastVerificationChildId = useIdeationStore(
    (s) => s.lastVerificationChildId[session?.id ?? ''] ?? null
  );
  const [displayedVerificationChildId, setDisplayedVerificationChildId] = useState<string | null>(null);
  const [displayedVerificationStatus, setDisplayedVerificationStatus] = useState<{
    status: VerificationStatus;
    inProgress: boolean;
  } | null>(null);

  // Poll status for verification child and direct child session views to detect pending_initial_prompt
  // Eagerly fetch the latest verification child id so chat routing is ready without
  // hydrating the full child-session history before the Verification tab needs it.
  const { data: latestVerificationChild } = useQuery({
    queryKey: ["childSessionId", session?.id, "verification", "latest"],
    queryFn: () =>
      ideationApi.sessions.getLatestChildSessionId(session!.id, "verification", {
        includeArchived: true,
      }),
    enabled: !!session?.id && session?.sessionPurpose !== "verification",
    staleTime: 30_000,
  });

  const latestVerificationChildId =
    latestVerificationChild?.latestChildSessionId ?? null;

  const verificationChatSessionId =
    displayedVerificationChildId ??
    lastVerificationChildId ??
    activeVerificationChildId ??
    latestVerificationChildId ??
    null;

  const { data: verificationChildStatus } = useChildSessionStatus(verificationChatSessionId);
  // Poll for the current session unconditionally — works for both child and top-level sessions.
  // The hook's historyMode guard will disable polling if the session is idle with no pending prompt.
  const { data: currentSessionStatus } = useChildSessionStatus(session?.id);

  // Pre-populate lastVerificationChildId from eager query result.
  // Only sets if store field is null (avoids overwriting event-driven updates).
  useEffect(() => {
    if (!session?.id || !latestVerificationChildId) return;
    if (lastVerificationChildId) return;

    setLastVerificationChildId(session.id, latestVerificationChildId);
  }, [
    session?.id,
    latestVerificationChildId,
    lastVerificationChildId,
    setLastVerificationChildId,
  ]);

  // Reset to plan tab when switching sessions
  const prevSessionIdRef = useRef<string | null>(null);
  useEffect(() => {
    if (!session?.id) return;
    if (prevSessionIdRef.current !== null && prevSessionIdRef.current !== session.id) {
      // Session changed — reset new session to plan tab, but preserve
      // per-session verification routing state so coming back stays reliable.
      setActiveIdeationTab(session.id, 'plan');
    }
    if (prevSessionIdRef.current !== session.id) {
      setDisplayedVerificationChildId(null);
      setDisplayedVerificationStatus(null);
    }
    prevSessionIdRef.current = session.id;
  }, [session?.id, setActiveIdeationTab]);

  const isVerificationTabActive = activeTab === 'verification';

  // Construct TeamMetadata when session is a team session
  const teamMetadata = useMemo<TeamMetadata | undefined>(() => {
    if (!session?.teamMode || session.teamMode === "solo") return undefined;
    return {
      teamIdeated: true,
      teamMode: session.teamMode as "research" | "debate",
      teammateCount: session.teamConfig?.maxTeammates ?? teamArtifacts.length,
      findings: [],
    };
  }, [session?.teamMode, session?.teamConfig?.maxTeammates, teamArtifacts.length]);

  // Verification tab visibility + badge state
  const rawVerificationStatus = session?.verificationStatus ?? "unverified";
  const hasKnownPlan = Boolean(effectivePlanId ?? planArtifact);
  const isTerminalVerificationStatus =
    rawVerificationStatus !== "unverified" && rawVerificationStatus !== "reviewing";
  const isVerificationActive = (session?.verificationInProgress ?? false) || !!activeVerificationChildId;
  const verificationStatus =
    !hasKnownPlan && !isVerificationActive && isTerminalVerificationStatus
      ? "unverified"
      : rawVerificationStatus;
  const effectiveVerificationStatus =
    displayedVerificationStatus?.status ?? verificationStatus;
  const effectiveVerificationInProgress =
    displayedVerificationStatus?.inProgress ?? isVerificationActive;
  const showVerificationTab = Boolean(
    effectiveVerificationStatus !== "unverified" || hasKnownPlan
  );
  const verificationBadge: "in_progress" | "verified" | "warning" | null = (() => {
    if (!session) return null;
    if (effectiveVerificationInProgress) return "in_progress";
    if (effectiveVerificationStatus === "verified" || effectiveVerificationStatus === "imported_verified") return "verified";
    if (effectiveVerificationStatus === "needs_revision") return "warning";
    return null;
  })();

  // Subscribe to plan:proposals_may_need_update — must stay mounted (not inside a conditional tab)
  useEffect(() => {
    const unsubProposalsUpdate = eventBus.subscribe<{ artifact_id: string; proposal_ids: string[]; session_id?: string }>(
      "plan:proposals_may_need_update",
      (payload) => {
        if (payload.session_id && session?.id && payload.session_id !== session.id) {
          return;
        }

        const affectedProposals = proposals.filter((p) => payload.proposal_ids.includes(p.id));
        const previousStates: Record<string, unknown> = {};
        affectedProposals.forEach((p) => { previousStates[p.id] = { ...p }; });

        showSyncNotification({
          artifactId: payload.artifact_id,
          proposalIds: payload.proposal_ids,
          previousStates,
          timestamp: Date.now(),
        });
      }
    );

    return () => { unsubProposalsUpdate(); };
  }, [eventBus, proposals, showSyncNotification, session?.id]);

  const handleResizeStart = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    setIsResizing(true);
  }, []);

  useEffect(() => {
    if (!isResizing) return;

    const handleMouseMove = (e: MouseEvent) => {
      if (!containerRef.current) return;
      const rect = containerRef.current.getBoundingClientRect();
      const newWidth = rect.right - e.clientX;
      setChatPanelWidth(Math.max(CHAT_PANEL_MIN_WIDTH, Math.min(600, newWidth)));
    };

    const handleMouseUp = () => setIsResizing(false);

    document.addEventListener("mousemove", handleMouseMove);
    document.addEventListener("mouseup", handleMouseUp);

    return () => {
      document.removeEventListener("mousemove", handleMouseMove);
      document.removeEventListener("mouseup", handleMouseUp);
    };
  }, [isResizing]);

  // Navigate to task handler
  const setCurrentView = useUiStore((state) => state.setCurrentView);
  const setSelectedTaskId = useUiStore((state) => state.setSelectedTaskId);

  // Get active project for feature branch setting
  const activeProject = useProjectStore(selectActiveProject);

  // Accept Plan — opens the confirmation modal
  const handleAcceptPlan = useCallback(() => {
    setIsAcceptModalOpen(true);
  }, []);

  // Handle confirmed accept from modal
  const handleAcceptConfirm = useCallback(async (options: ApplyProposalsInput) => {
    if (!session) return;
    const projectId = activeProjectId || session.projectId;
    if (!projectId) return;

    let executionPlanId: string | null | undefined;
    try {
      const applyResult = await onApply(options);
      if (!applyResult?.sessionConverted) {
        setIsAcceptModalOpen(false);
        return;
      }
      executionPlanId = applyResult?.executionPlanId;
    } catch (error) {
      const message = error instanceof Error ? error.message : "Failed to apply proposals";
      toast.error(message);
      return;
    }

    try {
      await setActivePlan(projectId, session.id, "ideation", executionPlanId);
    } catch (error) {
      console.error("Failed to set active plan:", error);
      toast.error("Failed to set active plan");
    }

    setIsAcceptModalOpen(false);
  }, [session, onApply, activeProjectId, setActivePlan]);

  const handleAcceptCancel = useCallback(() => {
    setIsAcceptModalOpen(false);
  }, []);

  const handleNavigateToTask = useCallback((taskId: string) => {
    setCurrentView("kanban");
    setSelectedTaskId(taskId);
  }, [setCurrentView, setSelectedTaskId]);

  const handleViewWork = useCallback(async () => {
    if (session) {
      const projectId = activeProjectId || session.projectId;
      if (projectId) {
        try {
          await setActivePlan(projectId, session.id, "ideation");
        } catch (error) {
          console.error("Failed to set active plan before navigating to graph:", error);
        }
      }
    }
    setCurrentView("graph");
  }, [session, activeProjectId, setActivePlan, setCurrentView]);

  const {
    highlightedProposalIds,
    isPlanExpanded,
    setIsPlanExpanded,
    importStatus,
    setImportStatus,
    fileInputRef,
    handleArchive,
    handleClearAll,
    handleReviewSync,
    handleUndoSync,
    handleDismissSync,
    handleImportPlan,
    handleFileSelected,
    handleFileDrop,
  } = useIdeationHandlers(
    session,
    proposals,
    onRemoveProposal,
    onReorderProposals,
    onArchiveSession,
    fetchPlanArtifact,
    dismissSyncNotification,
    syncNotification
  );

  // File drop hook for drag-and-drop markdown import
  const { isDragging, dropProps, error: fileDropError } = useFileDrop({
    acceptedExtensions: [".md"],
    onFileDrop: handleFileDrop,
    onError: (err) => setImportStatus({ type: "error", message: err.message }),
  });

  // Show file drop error in import status
  useEffect(() => {
    if (fileDropError) {
      setImportStatus({ type: "error", message: fileDropError.message });
      setTimeout(() => setImportStatus(null), 5000);
    }
  }, [fileDropError, setImportStatus]);

  // Auto-expand plan when there are no proposals
  const autoOpenedPlanRef = useRef(false);
  const userOverrideRef = useRef(false);

  useEffect(() => {
    if (userOverrideRef.current) return;
    if (isSessionLoading) return;
    if (planArtifact && proposals.length === 0 && !isPlanExpanded) {
      autoOpenedPlanRef.current = true;
      setIsPlanExpanded(true);
    }
  }, [planArtifact, proposals.length, isPlanExpanded, setIsPlanExpanded, isSessionLoading]);

  // Switch to Proposals tab when new proposal arrives
  const lastProposalAddedAt = useProposalStore((state) => session?.id ? (state.lastProposalAddedAt[session.id] ?? null) : null);
  const prevProposalAddedAtRef = useRef(lastProposalAddedAt);
  useEffect(() => {
    const changed = lastProposalAddedAt !== prevProposalAddedAtRef.current;
    prevProposalAddedAtRef.current = lastProposalAddedAt;
    if (!changed) return;
    if (userOverrideRef.current) return;
    if (lastProposalAddedAt !== null && isPlanExpanded && session) {
      autoOpenedPlanRef.current = false;
      setActiveIdeationTab(session.id, 'proposals');
    }
  }, [lastProposalAddedAt, isPlanExpanded, session, setActiveIdeationTab]);

  // If proposals load after an auto-open, switch to Proposals tab
  useEffect(() => {
    if (userOverrideRef.current) return;
    if (isSessionLoading) return;
    if (proposals.length > 0 && isPlanExpanded && autoOpenedPlanRef.current && session) {
      autoOpenedPlanRef.current = false;
      setActiveIdeationTab(session.id, 'proposals');
    }
  }, [proposals.length, isPlanExpanded, isSessionLoading, session, setActiveIdeationTab]);

  // Reset plan expansion when switching sessions
  const lastSessionIdRef = useRef<string | null>(null);
  useEffect(() => {
    if (!session?.id) return;
    if (lastSessionIdRef.current !== session.id) {
      lastSessionIdRef.current = session.id;
      autoOpenedPlanRef.current = false;
      userOverrideRef.current = false;
      prevProposalAddedAtRef.current = lastProposalAddedAt;
      lastDependencyToastAtRef.current = null;
      setIsPlanExpanded(!!(session.planArtifactId ?? session.inheritedPlanArtifactId));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [session?.id, session?.planArtifactId, session?.inheritedPlanArtifactId]);

  const handlePlanExpandedChange = useCallback((expanded: boolean) => {
    autoOpenedPlanRef.current = false;
    userOverrideRef.current = true;
    setIsPlanExpanded(expanded);
  }, [setIsPlanExpanded]);

  // Historical plan version — set when user clicks "View plan as of proposal creation (vX)"
  const [requestedHistoricalVersion, setRequestedHistoricalVersion] = useState<number | null>(null);

  const handleViewHistoricalPlan = useCallback((_artifactId: string, version: number) => {
    setRequestedHistoricalVersion(version);
    if (session) setActiveIdeationTab(session.id, 'plan');
    userOverrideRef.current = true;
    setIsPlanExpanded(true);
  }, [session, setActiveIdeationTab, setIsPlanExpanded]);

  const handleTabChange = useCallback((tab: 'plan' | 'proposals' | 'research') => {
    if (!session) return;
    setActiveIdeationTab(session.id, tab);
  }, [session, setActiveIdeationTab]);


  const handleVerificationTabClick = useCallback(async () => {
    if (!session) return;
    setActiveIdeationTab(session.id, 'verification');

    if (verificationChatSessionId) {
      if (!lastVerificationChildId) {
        setLastVerificationChildId(session.id, verificationChatSessionId);
      }
      if (isVerificationActive && !activeVerificationChildId) {
        setActiveVerificationChildId(session.id, verificationChatSessionId);
      }
    }

    // If child ID already preloaded (set by event handler), skip async fetch
    // But allow refetch when verification is actively running (state may have changed)
    if (verificationChatSessionId && !isVerificationActive) return;

    // Fetch the latest verification child id without hydrating the full history list.
    try {
      const latest = await ideationApi.sessions.getLatestChildSessionId(
        session.id,
        'verification',
        { includeArchived: true },
      );
      if (latest.latestChildSessionId) {
        // Only set activeVerificationChildId if not already populated (fresh hydration only)
        if (!activeVerificationChildId) {
          setActiveVerificationChildId(session.id, latest.latestChildSessionId);
        }
        setLastVerificationChildId(session.id, latest.latestChildSessionId);
      }
    } catch (err) {
      console.error('Verification tab: failed to fetch latest child session id', err);
      // Tab switches regardless — child panel stays hidden until child exists
    }
  }, [
    session,
    activeVerificationChildId,
    lastVerificationChildId,
    verificationChatSessionId,
    isVerificationActive,
    setActiveIdeationTab,
    setActiveVerificationChildId,
    setLastVerificationChildId,
  ]);

  const handleDisplayedVerificationStatusChange = useCallback(
    (status: VerificationStatus, inProgress: boolean) => {
      setDisplayedVerificationStatus({ status, inProgress });
    },
    [],
  );

  return (
    <>
      <style>{animationStyles}</style>
      <div
        ref={containerRef}
        data-testid="ideation-view"
        className="flex flex-col h-full relative"
        style={{ background: "var(--bg-base)" }}
        role="main"
      >
        <div className="flex flex-1 overflow-hidden">
          {/* Plan Browser Sidebar */}

          {/* Toggle strip — shown when collapsed (inline, not overlay) */}
          {isCollapsed && !isOverlayOpen && (
            <div
              role="button"
              aria-label="Open sidebar"
              tabIndex={0}
              data-testid="sidebar-toggle-strip"
              onClick={toggleCollapse}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") {
                  e.preventDefault();
                  toggleCollapse();
                }
              }}
              className="flex items-center justify-center shrink-0 cursor-pointer transition-colors duration-150"
              style={{
                width: 36,
                background: withAlpha("var(--bg-surface)", 50),
                borderRight: "1px solid var(--overlay-faint)",
                color: "var(--text-muted)",
              }}
              onMouseEnter={(e) => {
                e.currentTarget.style.background = "var(--overlay-weak)";
                e.currentTarget.style.color = "var(--text-primary)";
              }}
              onMouseLeave={(e) => {
                e.currentTarget.style.background = withAlpha("var(--bg-surface)", 50);
                e.currentTarget.style.color = "var(--text-muted)";
              }}
            >
              <Menu className="w-4 h-4" />
            </div>
          )}

          {/* Overlay backdrop */}
          {isOverlayOpen && (
            <div
              aria-hidden="true"
              onClick={closeOverlay}
              style={{
                position: "fixed",
                inset: 0,
                top: 48,
                background: "var(--overlay-scrim)",
                zIndex: 34,
              }}
            />
          )}

          {/* Inline sidebar column — kept mounted to preserve query cache */}
          <div
            ref={planBrowserSidebarRef}
            style={{
              width: isCollapsed && !isOverlayOpen ? 0 : effectiveSidebarWidth,
              minWidth: isCollapsed && !isOverlayOpen ? 0 : effectiveSidebarWidth,
              flexShrink: 0,
              overflow: "hidden",
              transition: planBrowserTransition,
              display: isCollapsed && !isOverlayOpen ? "none" : undefined,
            }}
            aria-hidden={isCollapsed && !isOverlayOpen ? "true" : undefined}
            data-testid="ideation-sidebar-container"
          >
            <PlanBrowser
              projectId={activeProjectId || session?.projectId || ""}
              currentPlanId={session?.id ?? null}
              onSelectPlan={onSelectSession}
              onNewPlan={onNewSession}
              onArchivePlan={onArchiveSession}
              onReopenPlan={(planId) => {
                onSelectSession(planId);
                handleOpenReopenDialog("reopen");
              }}
              onResetReacceptPlan={(planId) => {
                onSelectSession(planId);
                handleOpenReopenDialog("reset");
              }}
              width={effectiveSidebarWidth || 340}
              onCollapse={toggleCollapse}
            />
          </div>

          {showPlanBrowserResizeHandle && (
            <ResizeHandle
              isResizing={isPlanBrowserSidebarResizing}
              onMouseDown={handlePlanBrowserResizeStart}
              onDoubleClick={handlePlanBrowserResizeReset}
              testId="ideation-sidebar-resize-handle"
            />
          )}

          {/* Overlay sidebar */}
          {isOverlayOpen && (
            <div
              className="plan-browser-slide-in"
              style={{
                position: "fixed",
                top: 48,
                left: 0,
                height: "calc(100vh - 48px)",
                width: 340,
                zIndex: 35,
              }}
            >
              <PlanBrowser
                projectId={activeProjectId || session?.projectId || ""}
                currentPlanId={session?.id ?? null}
                onSelectPlan={(planId) => {
                  onSelectSession(planId);
                  closeOverlay();
                }}
                onNewPlan={() => {
                  onNewSession();
                  closeOverlay();
                }}
                onArchivePlan={onArchiveSession}
                onReopenPlan={(planId) => {
                  onSelectSession(planId);
                  handleOpenReopenDialog("reopen");
                  closeOverlay();
                }}
                onResetReacceptPlan={(planId) => {
                  onSelectSession(planId);
                  handleOpenReopenDialog("reset");
                  closeOverlay();
                }}
                width={340}
                onCollapse={closeOverlay}
              />
            </div>
          )}

          {/* Main Content - Column layout for session or no-session */}
          <div className="flex flex-col flex-1 overflow-hidden">
            {/* Header - Only shown when session is active */}
            {session && (
            <header
              data-testid="ideation-header"
              className="flex items-center justify-between h-14 px-4 border-b"
              style={{
                borderColor: "var(--overlay-faint)",
                background: withAlpha("var(--bg-surface)", 85),
                backdropFilter: "blur(20px)",
                WebkitBackdropFilter: "blur(20px)",
              }}
            >
              <div className="flex items-center gap-2">
                {/* Parent session breadcrumb */}
                {session.parentSessionId && (() => {
                  const parentSession = allSessionsForBreadcrumb.find((s) => s.id === session.parentSessionId);
                  if (parentSession) {
                    return (
                      <button
                        onClick={() => onSelectSession(session.parentSessionId!)}
                        className="flex items-center gap-1.5 px-2 py-1 rounded-md transition-colors duration-150"
                        style={{
                          background: "var(--overlay-faint)",
                          border: "1px solid var(--overlay-weak)",
                        }}
                        onMouseEnter={(e) => {
                          e.currentTarget.style.background = "var(--overlay-weak)";
                        }}
                        onMouseLeave={(e) => {
                          e.currentTarget.style.background = "var(--overlay-faint)";
                        }}
                      >
                        <ArrowLeft className="w-3 h-3" style={{ color: "var(--text-secondary)" }} />
                        <span className="text-[0.6875rem]" style={{ color: "var(--text-secondary)" }}>
                          {parentSession.title || "Untitled"}
                        </span>
                      </button>
                    );
                  }
                  return null;
                })()}
                <div>
                  <h1
                    className="text-[0.875rem] font-semibold tracking-tight leading-tight"
                    style={{ color: "var(--text-primary)" }}
                  >
                    {session.title || "New Session"}
                  </h1>
                  <p
                    className="text-[0.75rem]"
                    style={{ color: "var(--text-muted)" }}
                  >
                    {proposals.length} {proposals.length === 1 ? "proposal" : "proposals"}
                  </p>
                  {(session.sourceTaskId || session.spawnReason || session.sourceProjectId || session.sourceSessionId) && (
                    <div
                      className="flex flex-wrap items-center gap-x-2 gap-y-0.5 mt-0.5 text-[0.625rem]"
                      style={{ color: "var(--text-secondary)" }}
                    >
                      {session.sourceTaskId && (
                        <span>
                          {sourceContextLabel || "Follow-up"}: {sourceTaskTitle || session.sourceTaskId}
                        </span>
                      )}
                      {!session.sourceTaskId && session.sourceSessionId && (
                        <span>Source session: {session.sourceSessionId}</span>
                      )}
                      {session.sourceProjectId && (
                        <span>Source project: {session.sourceProjectId}</span>
                      )}
                      {session.spawnReason && (
                        <span>Reason: {session.spawnReason.split("_").join(" ")}</span>
                      )}
                    </div>
                  )}
                </div>
              </div>
              <div className="flex items-center gap-1.5">
                {canReopen && (
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => handleOpenReopenDialog("reopen")}
                    className="h-7 gap-1.5 text-xs transition-colors duration-150"
                    style={{ color: "var(--text-secondary)" }}
                    onMouseEnter={(e) => {
                      e.currentTarget.style.color = "var(--text-primary)";
                      e.currentTarget.style.background = "var(--overlay-weak)";
                    }}
                    onMouseLeave={(e) => {
                      e.currentTarget.style.color = "var(--text-secondary)";
                      e.currentTarget.style.background = "transparent";
                    }}
                  >
                    <RotateCcw className="w-3.5 h-3.5" />
                    Reopen
                  </Button>
                )}
                {canReopen && canResetReaccept && (
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => handleOpenReopenDialog("reset")}
                    className="h-7 gap-1.5 text-xs transition-colors duration-150"
                    style={{ color: "var(--text-secondary)" }}
                    onMouseEnter={(e) => {
                      e.currentTarget.style.color = "var(--text-primary)";
                      e.currentTarget.style.background = "var(--overlay-weak)";
                    }}
                    onMouseLeave={(e) => {
                      e.currentTarget.style.color = "var(--text-secondary)";
                      e.currentTarget.style.background = "transparent";
                    }}
                  >
                    <RefreshCw className="w-3.5 h-3.5" />
                    Reset & Re-accept
                  </Button>
                )}
                {!isReadOnly && (
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={handleArchive}
                    className="h-7 gap-1.5 text-xs transition-colors duration-150"
                    style={{ color: "var(--text-secondary)" }}
                    onMouseEnter={(e) => {
                      e.currentTarget.style.color = "var(--text-primary)";
                      e.currentTarget.style.background = "var(--overlay-weak)";
                    }}
                    onMouseLeave={(e) => {
                      e.currentTarget.style.color = "var(--text-secondary)";
                      e.currentTarget.style.background = "transparent";
                    }}
                  >
                    <Archive className="w-3.5 h-3.5" />
                    Archive
                  </Button>
                )}
              </div>
            </header>
          )}

          {/* Split Layout - Left section with footer support, Conversation right */}
          <div data-testid="ideation-main-content" className="flex flex-1 overflow-hidden">
            {/* Left Section - Column layout with tab content and optional footer */}
            <div className="flex flex-col flex-1 min-w-0 overflow-hidden">
              {/* Main Content Area - Session or No-Session */}
              <div
                data-testid="proposals-panel"
                className="flex flex-col relative flex-1 min-h-0"
                style={{
                  minWidth: "360px",
                  borderRight: "1px solid var(--overlay-faint)",
                  background: "var(--bg-base)",
                }}
                {...(session ? dropProps : {})}
              >
                {/* Drop zone overlay - shown during drag (active session only) */}
                {session && <DropZoneOverlay isVisible={isDragging} message="Drop to import plan" />}

                {/* No-Session State */}
                {!session && (
                  <StartSessionPanel onNewSession={onNewSession} />
                )}

                {/* Active-Session State */}
                {session && (
                  <>
                    {/* Tab bar — Plan | Proposals | Team Research (conditional).
                        Height matches the right-side chat header (h-11 = 44px) so
                        the horizontal divider unites across the split pane. */}
                    <div
                      className="flex items-center gap-0 px-4 shrink-0"
                      style={{
                        height: "44px",
                        borderBottom: "1px solid var(--overlay-faint)",
                        background: withAlpha("var(--bg-surface)", 60),
                        backdropFilter: "blur(12px)",
                        WebkitBackdropFilter: "blur(12px)",
                      }}
                      data-testid="content-tab-bar"
                    >
                      <button
                        onClick={() => handleTabChange('plan')}
                        className="relative h-full px-3 text-[0.75rem] font-medium transition-colors duration-150"
                        style={{
                          color: activeTab === "plan"
                            ? "var(--text-primary)"
                            : "var(--text-muted)",
                        }}
                        data-testid="tab-plan"
                      >
                        Plan
                        {activeTab === "plan" && (
                          <span
                            className="absolute bottom-0 left-3 right-3 h-[2px] rounded-full"
                            style={{ background: "var(--accent-primary)" }}
                          />
                        )}
                      </button>
                      {showVerificationTab && (
                        <button
                          onClick={handleVerificationTabClick}
                          className="relative h-full px-3 text-[0.75rem] font-medium transition-colors duration-150 flex items-center gap-1.5"
                          style={{
                            color: activeTab === "verification"
                              ? "var(--text-primary)"
                              : "var(--text-muted)",
                          }}
                          data-testid="tab-verification"
                        >
                          Verification
                          {verificationBadge === "in_progress" && (
                            <span
                              data-testid="verification-badge-in-progress"
                              className="w-2 h-2 rounded-full animate-pulse shrink-0"
                              style={{ background: "var(--status-warning)" }}
                            />
                          )}
                          {verificationBadge === "verified" && (
                            <CheckCircle
                              data-testid="verification-badge-verified"
                              className="w-3 h-3 shrink-0"
                              style={{ color: "var(--status-success)" }}
                            />
                          )}
                          {verificationBadge === "warning" && (
                            <AlertTriangle
                              data-testid="verification-badge-warning"
                              className="w-3 h-3 shrink-0"
                              style={{ color: "var(--status-warning)" }}
                            />
                          )}
                          {activeTab === "verification" && (
                            <span
                              className="absolute bottom-0 left-3 right-3 h-[2px] rounded-full"
                              style={{ background: "var(--accent-primary)" }}
                            />
                          )}
                        </button>
                      )}
                      <button
                        onClick={() => handleTabChange('proposals')}
                        className="relative h-full px-3 text-[0.75rem] font-medium transition-colors duration-150 flex items-center gap-1.5"
                        style={{
                          color: activeTab === "proposals"
                            ? "var(--text-primary)"
                            : "var(--text-muted)",
                        }}
                        data-testid="tab-proposals"
                      >
                        Proposals
                        {proposals.length > 0 && (
                          <span
                            className="text-[0.625rem] font-semibold px-1.5 py-0.5 rounded-full"
                            style={{
                              background: activeTab === "proposals"
                                ? withAlpha("var(--accent-primary)", 15)
                                : "var(--overlay-weak)",
                              color: activeTab === "proposals"
                                ? "var(--accent-primary)"
                                : "var(--text-muted)",
                            }}
                          >
                            {proposals.length}
                          </span>
                        )}
                        {activeTab === "proposals" && (
                          <span
                            className="absolute bottom-0 left-3 right-3 h-[2px] rounded-full"
                            style={{ background: "var(--accent-primary)" }}
                          />
                        )}
                      </button>
                      {teamArtifacts.length > 0 && (
                        <button
                          onClick={() => handleTabChange('research')}
                          className="relative h-full px-3 text-[0.75rem] font-medium transition-colors duration-150 flex items-center gap-1.5"
                          style={{
                            color: activeTab === "research"
                              ? "var(--text-primary)"
                              : "var(--text-muted)",
                          }}
                          data-testid="tab-research"
                        >
                          Team Research
                          <span
                            className="text-[0.625rem] font-semibold px-1.5 py-0.5 rounded-full"
                            style={{
                              background: activeTab === "research"
                                ? withAlpha("var(--accent-primary)", 15)
                                : "var(--overlay-weak)",
                              color: activeTab === "research"
                                ? "var(--accent-primary)"
                                : "var(--text-muted)",
                            }}
                          >
                            {teamArtifacts.length}
                          </span>
                          {activeTab === "research" && (
                            <span
                              className="absolute bottom-0 left-3 right-3 h-[2px] rounded-full"
                              style={{ background: "var(--accent-primary)" }}
                            />
                          )}
                        </button>
                      )}
                    </div>

                    {/* Tab content — delegates to extracted components */}
                    {activeTab === "plan" && (
                      <PlanTabContent
                        session={session}
                        proposals={proposals}
                        {...(teamMetadata !== undefined && { teamMetadata })}
                        importStatus={importStatus}
                        onImportStatusChange={setImportStatus}
                        onImportPlan={handleImportPlan}
                        onViewWork={handleViewWork}
                        isPlanExpanded={isPlanExpanded}
                        onExpandedChange={handlePlanExpandedChange}
                        requestedHistoricalVersion={requestedHistoricalVersion}
                        onHistoricalVersionViewed={() => setRequestedHistoricalVersion(null)}
                      />
                    )}

                    {activeTab === "verification" && (
                      <div
                        data-testid="verification-tab-content"
                        className="flex flex-col flex-1 min-h-0"
                      >
                        <VerificationPanel
                          session={session}
                          onDisplayedVerificationChildChange={setDisplayedVerificationChildId}
                          onDisplayedVerificationStatusChange={handleDisplayedVerificationStatusChange}
                        />
                      </div>
                    )}

                    {activeTab === "proposals" && (
                      <ProposalsTabContent
                        session={session}
                        proposals={proposals}
                        dependencyGraph={dependencyGraph}
                        criticalPathSet={criticalPathSet}
                        highlightedIds={highlightedProposalIds}
                        isReadOnly={isReadOnly}
                        onEditProposal={onEditProposal}
                        onNavigateToTask={handleNavigateToTask}
                        onViewHistoricalPlan={handleViewHistoricalPlan}
                        {...(onViewProposal !== undefined && { onViewProposal })}
                        {...(selectedProposalId !== undefined && { selectedProposalId })}
                        onImportPlan={handleImportPlan}
                        onClearAll={handleClearAll}
                        onAcceptPlan={handleAcceptPlan}
                        onReviewSync={handleReviewSync}
                        onUndoSync={handleUndoSync}
                        onDismissSync={handleDismissSync}
                        onDeleteProposal={onRemoveProposal}
                      />
                    )}

                    {activeTab === "research" && (
                      <TeamResearchTabContent
                        teamArtifacts={teamArtifacts}
                        sessionId={session.id}
                      />
                    )}
                  </>
                )}
              </div>

              {/* Footer Region - renders footer when provided */}
              {footer && (
                <div className="flex-shrink-0" data-testid="ideation-footer">
                  {footer}
                </div>
              )}
            </div>

            {/* Resize Handle - only when session active */}
            {session && (
            <ResizeHandle
              isResizing={isResizing}
              onMouseDown={handleResizeStart}
              testId="ideation-resize-handle"
            />
            )}

            {/* Conversation Panel (Right) - Only shown when session is active */}
            {session && (
            <div
              data-testid="conversation-panel"
              className="flex flex-col shrink-0"
              style={{ width: `${chatPanelWidth}px` }}
            >
              {/* Parent chat panel — always mounted, hidden when showing verification child */}
              <div
                className="flex flex-col flex-1"
                style={{ display: (!isVerificationTabActive || !verificationChatSessionId) ? 'flex' : 'none' }}
              >
                <IntegratedChatPanel
                  key={session.id}
                  projectId={session.projectId}
                  ideationSessionId={session.id}
                  emptyState={currentSessionStatus?.pending_initial_prompt ? <WaitingForCapacityState pendingInitialPrompt={currentSessionStatus.pending_initial_prompt} projectId={session.projectId} /> : <ConversationEmptyState />}
                  showHelperTextAlways={true}
                  isVisible={!isVerificationTabActive || !verificationChatSessionId}
                  headerContent={
                    <div className="flex items-center gap-2 min-w-0 flex-1">
                      <MessageSquare className="w-3.5 h-3.5 shrink-0" style={{ color: "var(--text-muted)" }} />
                      <span className="text-[0.8125rem] font-medium" style={{ color: "var(--text-primary)" }}>Conversation</span>
                    </div>
                  }
                />
              </div>
              {/* Verification child chat panel — mounted only when child session exists */}
              {verificationChatSessionId && (
                <div
                  className="flex flex-col flex-1"
                  style={{
                    display: isVerificationTabActive ? 'flex' : 'none',
                    borderLeft: '2px solid var(--status-warning-border)',
                  }}
                >
                  <IntegratedChatPanel
                    key={verificationChatSessionId}
                    projectId={session.projectId}
                    ideationSessionId={verificationChatSessionId}
                    emptyState={verificationChildStatus?.pending_initial_prompt ? <WaitingForCapacityState pendingInitialPrompt={verificationChildStatus.pending_initial_prompt} projectId={session.projectId} /> : <ConversationEmptyState />}
                    showHelperTextAlways={true}
                    isVisible={isVerificationTabActive}
                    toolbarBackAction={{ label: 'Plan', icon: <ArrowLeft className="w-3 h-3" />, onClick: () => setActiveIdeationTab(session.id, 'plan') }}
                    headerContent={
                      <div className="flex items-center gap-2 min-w-0 flex-1">
                        <ShieldCheck className="w-3.5 h-3.5 shrink-0" style={{ color: "var(--status-warning)" }} />
                        <div className="flex flex-col min-w-0 flex-1">
                          <span className="text-[0.6875rem] font-semibold leading-tight truncate" style={{ color: "var(--status-warning)" }}>
                            Verification
                          </span>
                          <span className="text-[0.625rem] leading-tight truncate" style={{ color: "var(--text-muted)" }}>
                            {session.title || "Untitled"}
                          </span>
                        </div>
                      </div>
                    }
                  />
                </div>
              )}
            </div>
            )}
          </div>
          </div>
        </div>

        <input
          ref={fileInputRef}
          type="file"
          accept=".md"
          onChange={handleFileSelected}
          className="hidden"
          data-testid="plan-import-file-input"
        />
      </div>

      <ReopenSessionDialog
        open={reopenDialogOpen}
        onOpenChange={setReopenDialogOpen}
        mode={reopenDialogMode}
        sessionTitle={session?.title || "Untitled"}
        taskCount={sessionTaskCount}
        onConfirm={handleConfirmReopen}
        isLoading={reopenMutation.isPending || resetMutation.isPending}
        {...(planBranch?.branchName !== undefined && { featureBranch: planBranch.branchName })}
        {...(planBranch?.baseBranchOverride != null && { targetBranch: planBranch.baseBranchOverride })}
      />

      {/* Accept Plan Confirmation Modal */}
      {session && dependencyGraph && (
        <AcceptModal
          isOpen={isAcceptModalOpen}
          proposals={proposals}
          dependencyGraph={dependencyGraph}
          sessionId={session.id}
          onAccept={handleAcceptConfirm}
          onCancel={handleAcceptCancel}
          session={session}
          workingDirectory={activeProject?.workingDirectory}
          baseBranch={activeProject?.baseBranch ?? "main"}
        />
      )}
    </>
  );
}

// Backward compatibility alias
export { PlanningView as IdeationView };
