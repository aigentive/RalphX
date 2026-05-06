/**
 * RalphX - App Shell
 * Root component with QueryClientProvider and EventProvider
 */

import { useMemo, useState, useEffect, useCallback, useRef } from "react";
import { useShallow } from "zustand/react/shallow";
import { QueryClientProvider } from "@tanstack/react-query";
import { toast } from "sonner";
import { getQueryClient } from "@/lib/queryClient";
import { EventProvider } from "@/providers/EventProvider";
import { TaskBoard } from "@/components/tasks/TaskBoard";
import { ReviewsPanel } from "@/components/reviews/ReviewsPanel";
import { ExecutionControlBar } from "@/components/execution/ExecutionControlBar";
import { AppTopBar, KanbanSplitLayout, LeftNavRail } from "@/components/layout";
import { PermissionDialog } from "@/components/PermissionDialog";
import { IdeationView, ProposalEditModal, FinalizeConfirmationDialog, VerificationConfirmDialog } from "@/components/Ideation";
import { ProposalDetailSheet } from "@/components/Ideation/ProposalDetailSheet";
import type { ProposalDetailEnrichment } from "@/components/Ideation/ProposalDetailSheet";
import { ExtensibilityView } from "@/components/ExtensibilityView";
import { ActivityView } from "@/components/activity";
import SettingsDialog from "@/components/settings/SettingsDialog";
import { InsightsView } from "@/components/views/InsightsView";
import { AgentsView } from "@/components/agents";
import { TeamSplitView } from "@/components/Team";
import { TaskGraphView } from "@/components/TaskGraph";
import { WelcomeScreen } from "@/components/WelcomeScreen";
import { UpdateChecker } from "@/components/UpdateChecker";
import { ProjectCreationWizard } from "@/components/projects/ProjectCreationWizard";
import { PlanQuickSwitcherPalette } from "@/components/plan/PlanQuickSwitcherPalette";
import { useUiStore } from "@/stores/uiStore";
import { useTaskStore, selectTasksByStatus } from "@/stores/taskStore";
import { useChatStore } from "@/stores/chatStore";
import { useIdeationStore, selectActiveSession } from "@/stores/ideationStore";
import { useProposalStore } from "@/stores/proposalStore";
import { useProjectStore } from "@/stores/projectStore";
import { useAgentSessionStore } from "@/stores/agentSessionStore";
import { DEFAULT_PROJECT_VIEW, type ViewType } from "@/types/chat";
import type { ApplyProposalsInput } from "@/api/ideation.types";
import type { UpdateProposalInput } from "@/api/ideation";
import { toTaskProposal, ideationApi } from "@/api/ideation";
import type { CreateProject } from "@/types/project";
import { useTasksAwaitingReview } from "@/hooks/useReviews";
import { useReviewMutations } from "@/hooks/useReviewMutations";
import { useExecutionEvents } from "@/hooks/useExecutionEvents";
import { useExecutionStatus } from "@/hooks/useExecutionControl";
import { useRunningProcesses } from "@/hooks/useRunningProcesses";
import { useMergePipeline } from "@/hooks/useMergePipeline";
import { useProjects, projectKeys } from "@/hooks/useProjects";
import {
  useIdeationSession,
  useIdeationSessions,
  useArchiveIdeationSession,
} from "@/hooks/useIdeation";
import { useProposalMutations } from "@/hooks/useProposals";
import { useApplyProposals } from "@/hooks/useApplyProposals";
import { useAppKeyboardShortcuts } from "@/hooks/useAppKeyboardShortcuts";
import { useFeatureFlags, isViewEnabled } from "@/hooks/useFeatureFlags";
import { useNavCompactBreakpoint } from "@/hooks";
import { extractErrorMessage } from "@/lib/errors";
import { resolveIdeationSession } from "@/lib/resolveIdeationSession";
import { api, getGitBranches, getGitDefaultBranch } from "@/lib/tauri";
import { executionApi } from "@/api/execution";
import { tasksApi } from "@/api/tasks";
import type { SelectionSource } from "@/api/plan";
import type { ProjectSettings } from "@/types/settings";
import { DEFAULT_PROJECT_SETTINGS } from "@/types/settings";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { TooltipProvider } from "@/components/ui/tooltip";
import { Toaster } from "@/components/ui/sonner";
import { ScreenshotGalleryTestPage } from "@/test-pages/ScreenshotGalleryTest";

const queryClient = getQueryClient();

/**
 * Test page router - checks URL params and returns test page if applicable
 * This is extracted to avoid hooks being called after conditional returns
 */
function getTestPage(): React.ReactElement | null {
  if (typeof window === "undefined") return null;

  const params = new URLSearchParams(window.location.search);
  const testPage = params.get("test");
  const scenario = params.get("scenario") || "default";

  if (testPage === "screenshot-gallery") {
    const scenarios: Record<string, React.ReactElement> = {
      default: <ScreenshotGalleryTestPage />,
      empty: <ScreenshotGalleryTestPage screenshots={[]} />,
      twoColumns: <ScreenshotGalleryTestPage columns={2} />,
      fourColumns: <ScreenshotGalleryTestPage columns={4} />,
    };
    return scenarios[scenario] ?? scenarios.default ?? null;
  }

  return null;
}

function FeatureDisabledPlaceholder({
  view,
  yamlKey,
  envVar,
}: {
  view: string;
  yamlKey: string;
  envVar: string;
}) {
  return (
    <div
      className="flex flex-col items-center justify-center h-full gap-4 p-8 text-center"
      data-testid={`feature-disabled-${view}`}
    >
      <p className="text-sm font-semibold" style={{ color: "var(--text-primary)" }}>
        {view} page is disabled (dev mode)
      </p>
      <div className="text-xs font-mono rounded p-3 text-left" style={{ background: "var(--bg-surface)", color: "var(--text-secondary)" }}>
        <p className="mb-2 font-sans" style={{ color: "var(--text-muted)" }}>Enable via ralphx.yaml:</p>
        <pre>{`ui:\n  feature_flags:\n    ${yamlKey}: true`}</pre>
        <p className="mt-3 mb-1 font-sans" style={{ color: "var(--text-muted)" }}>Or via env var:</p>
        <pre>{`${envVar}=true`}</pre>
      </div>
    </div>
  );
}

function AppContent() {
  // Check for test page first (must happen before any hooks for ESLint compliance)
  const testPage = useMemo(() => getTestPage(), []);

  const reviewsPanelOpen = useUiStore((s) => s.reviewsPanelOpen);
  const toggleReviewsPanel = useUiStore((s) => s.toggleReviewsPanel);
  const setReviewsPanelOpen = useUiStore((s) => s.setReviewsPanelOpen);
  const executionStatus = useUiStore((s) => s.executionStatus);
  const setExecutionStatus = useUiStore((s) => s.setExecutionStatus);
  const currentView = useUiStore((s) => s.currentView);
  const setCurrentView = useUiStore((s) => s.setCurrentView);
  const setSelectedTaskId = useUiStore((s) => s.setSelectedTaskId);
  const toggleGraphRightPanelUserOpen = useUiStore((s) => s.toggleGraphRightPanel);
  const toggleGraphRightPanelCompactOpen = useUiStore(
    (s) => s.toggleGraphRightPanelCompactOpen
  );
  const openModal = useUiStore((s) => s.openModal);
  const battleModeActive = useUiStore((s) => s.battleModeActive);
  const enterBattleMode = useUiStore((s) => s.enterBattleMode);
  const exitBattleMode = useUiStore((s) => s.exitBattleMode);
  const { isNavCompact } = useNavCompactBreakpoint();
  const { data: featureFlags } = useFeatureFlags();

  // Redirect to the default project view in production when the current view is disabled.
  useEffect(() => {
    if (!import.meta.env.DEV && !isViewEnabled(currentView, featureFlags)) {
      setCurrentView(DEFAULT_PROJECT_VIEW);
    }
  }, [currentView, featureFlags, setCurrentView]);

  // Welcome screen overlay state
  const showWelcomeOverlay = useUiStore((s) => s.showWelcomeOverlay);
  const welcomeOverlayReturnView = useUiStore((s) => s.welcomeOverlayReturnView);
  const openWelcomeOverlay = useUiStore((s) => s.openWelcomeOverlay);
  const closeWelcomeOverlay = useUiStore((s) => s.closeWelcomeOverlay);
  // Activity filter state (for context-aware navigation from StatusActivityBadge)
  const activityFilter = useUiStore((s) => s.activityFilter);

  // Chat message management for embedded ideation/agent conversations.
  const clearMessages = useChatStore((s) => s.clearMessages);

  const switchToProject = useUiStore((s) => s.switchToProject);

  // Project state
  const activeProjectId = useProjectStore((s) => s.activeProjectId);
  const setProjects = useProjectStore((s) => s.setProjects);
  const addProject = useProjectStore((s) => s.addProject);
  const selectProject = useProjectStore((s) => s.selectProject);
  const clearAgentSelection = useAgentSessionStore((s) => s.clearSelection);
  const setFocusedAgentProject = useAgentSessionStore((s) => s.setFocusedProject);

  const prevProjectIdRef = useRef<string | null>(null);
  const agentsReturnViewRef = useRef<ViewType>(DEFAULT_PROJECT_VIEW);

  // Fetch projects from backend
  const { data: fetchedProjects, isLoading: isLoadingProjects } = useProjects();

  // Project creation wizard state
  const [isProjectWizardOpen, setIsProjectWizardOpen] = useState(false);
  const [isCreatingProject, setIsCreatingProject] = useState(false);
  const [projectCreationError, setProjectCreationError] = useState<string | null>(null);

  // Plan quick switcher state
  const [isPlanQuickSwitcherOpen, setIsPlanQuickSwitcherOpen] = useState(false);
  const [planQuickSwitcherSource, setPlanQuickSwitcherSource] =
    useState<SelectionSource>("quick_switcher");

  // Ideation state
  const activeSession = useIdeationStore(selectActiveSession);
  const setActiveSession = useIdeationStore((s) => s.setActiveSession);
  const selectSession = useIdeationStore((s) => s.selectSession);
  const archiveSessionInStore = useIdeationStore((s) => s.archiveSession);
  const activeSessionId = activeSession?.id ?? "";
  // Get raw proposals from store and memoize the filtered/sorted version
  const allProposals = useProposalStore((s) => s.proposals);
  const setProposals = useProposalStore((s) => s.setProposals);
  const proposals = useMemo(() => {
    if (!activeSessionId) return [];
    return Object.values(allProposals)
      .filter((p) => p.sessionId === activeSessionId)
      .sort((a, b) => a.sortOrder - b.sortOrder);
  }, [allProposals, activeSessionId]);
  const [editingProposalId, setEditingProposalId] = useState<string | null>(null);
  const editingProposal = editingProposalId
    ? allProposals[editingProposalId] ?? null
    : null;

  const [viewingProposalId, setViewingProposalId] = useState<string | null>(null);
  const [viewingEnrichment, setViewingEnrichment] = useState<ProposalDetailEnrichment | undefined>(undefined);
  const viewingProposal = viewingProposalId
    ? allProposals[viewingProposalId] ?? null
    : null;

  const [isExecutionLoading, setIsExecutionLoading] = useState(false);

  // Execution settings state (persisted to database)
  const [executionSettings, setExecutionSettings] = useState<ProjectSettings | null>(null);

  // Running processes data for popover
  const { data: runningProcessesData } = useRunningProcesses(activeProjectId ?? undefined);
  const [isLoadingSettings, setIsLoadingSettings] = useState(true);
  const [isSavingSettings, setIsSavingSettings] = useState(false);
  const [settingsError, setSettingsError] = useState<string | null>(null);
  const saveTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Check if we should show the empty state (no projects)
  // Use TanStack Query data directly — the Zustand store sync via useEffect
  // can lag behind, causing a brief flash where store.projects is {} while
  // fetchedProjects already has data.
  const hasNoProjects = !isLoadingProjects && (!fetchedProjects || fetchedProjects.length === 0);

  // Use active project ID (queries are disabled when null)
  const currentProjectId = activeProjectId ?? "";

  const { totalCount: pendingReviewCount } = useTasksAwaitingReview(currentProjectId);

  // Real-time execution status updates via Tauri events
  useExecutionEvents();
  // Fetch initial execution status and poll every 30s as fallback
  // Pass currentProjectId for per-project execution status scoping
  useExecutionStatus(currentProjectId || undefined);
  const { isApproving, isRequestingChanges } = useReviewMutations();

  // Merge pipeline data
  const { data: mergePipelineData } = useMergePipeline(activeProjectId ?? undefined);
  const mergingCount = useMemo(() => {
    if (!mergePipelineData) return 0;
    return mergePipelineData.active.length + mergePipelineData.waiting.length;
  }, [mergePipelineData]);
  const mergeAttentionCount = useMemo(() => {
    return mergePipelineData?.needsAttention.length ?? 0;
  }, [mergePipelineData]);
  const hasAttentionMerges = useMemo(() => {
    return mergeAttentionCount > 0;
  }, [mergeAttentionCount]);

  // Paused tasks (provider errors)
  // useShallow prevents infinite re-renders: selectTasksByStatus returns a new array
  // on every call via .filter(), and Zustand's default Object.is sees new !== old.
  const pausedTasks = useTaskStore(useShallow(selectTasksByStatus("paused")));
  const pausedCount = pausedTasks.length;

  // Ideation hooks
  const { data: sessionData, isLoading: isSessionLoading } = useIdeationSession(activeSession?.id ?? "");
  const { data: allSessions = [] } = useIdeationSessions(currentProjectId);
  const archiveSession = useArchiveIdeationSession();
  const { deleteProposal, reorder, updateProposal } = useProposalMutations();
  const { apply: applyProposalsMutation } = useApplyProposals();

  const resolvedSession = useMemo(() => {
    return resolveIdeationSession(sessionData?.session, activeSession);
  }, [sessionData?.session, activeSession]);

  // Mirror PlanningView's isReadOnly: sessions that are not "active" are read-only
  const isIdeationReadOnly = resolvedSession?.status !== "active";

  // Sync proposals from sessionData to the store
  useEffect(() => {
    if (sessionData?.proposals) {
      // Convert API response to store type using proper mapping function
      setProposals(sessionData.proposals.map(toTaskProposal));
    }
  }, [sessionData?.proposals, setProposals]);


  // Sync fetched projects to store and auto-select first project
  useEffect(() => {
    if (fetchedProjects && fetchedProjects.length > 0) {
      setProjects(fetchedProjects);
      // Auto-select first project if none is selected
      if (!activeProjectId) {
        const firstProject = fetchedProjects[0];
        if (firstProject) {
          selectProject(firstProject.id);
        }
      }
    }
  }, [fetchedProjects, setProjects, activeProjectId, selectProject]);

  // Phase 82: Notify backend of active project changes for scoped execution
  // Only send when we have an actual project ID — skip the initial null
  // that occurs before Zustand persist hydration from localStorage.
  useEffect(() => {
    if (activeProjectId) {
      executionApi.setActiveProject(activeProjectId).catch((err) => {
        console.error("Failed to set active project:", err);
      });
    }
  }, [activeProjectId]);

  // Project switch: save/restore per-project view + ideation session
  // Runs AFTER the setActiveProject backend sync effect (order matters in React)
  useEffect(() => {
    const prevId = prevProjectIdRef.current;
    prevProjectIdRef.current = activeProjectId;

    if (prevId !== activeProjectId && activeProjectId) {
      // Atomic view state save/clean/restore
      switchToProject(prevId, activeProjectId);

      // Restore ideation session (separate store, same synchronous tick)
      const sessionByProject = useUiStore.getState().sessionByProject;
      const restoredSessionId = sessionByProject[activeProjectId] ?? null;

      if (restoredSessionId) {
        const sessions = useIdeationStore.getState().sessions;
        if (sessions[restoredSessionId]) {
          setActiveSession(restoredSessionId);
        } else {
          // Session was deleted/not yet loaded — don't restore stale ID
          setActiveSession(null);
        }
      } else {
        setActiveSession(null);
      }
    }
  }, [activeProjectId, switchToProject, setActiveSession]);

  // Load execution settings from database when project changes
  useEffect(() => {
    async function loadSettings() {
      try {
        setIsLoadingSettings(true);
        setSettingsError(null);
        // Phase 82: Pass currentProjectId for per-project settings
        const response = await executionApi.getSettings(currentProjectId || undefined);
        // Map API response (camelCase) to settings type (snake_case)
        setExecutionSettings({
          ...DEFAULT_PROJECT_SETTINGS,
          execution: {
            ...DEFAULT_PROJECT_SETTINGS.execution,
            max_concurrent_tasks: response.maxConcurrentTasks,
            project_ideation_max: response.projectIdeationMax,
            auto_commit: response.autoCommit,
            pause_on_failure: response.pauseOnFailure,
          },
        });
      } catch (err) {
        console.error("Failed to load execution settings:", err);
        setSettingsError(err instanceof Error ? err.message : "Failed to load settings");
        // Fall back to defaults
        setExecutionSettings(DEFAULT_PROJECT_SETTINGS);
      } finally {
        setIsLoadingSettings(false);
      }
    }
    loadSettings();
  }, [currentProjectId]);

  // Debounced handler for execution settings changes (300ms)
  const handleSettingsChange = useCallback((newSettings: ProjectSettings) => {
    // Update local state immediately for responsive UI
    setExecutionSettings(newSettings);
    setSettingsError(null);

    // Clear any pending save
    if (saveTimeoutRef.current) {
      clearTimeout(saveTimeoutRef.current);
    }

    // Debounce the API call
    saveTimeoutRef.current = setTimeout(async () => {
      try {
        setIsSavingSettings(true);
        // Phase 82: Pass currentProjectId for per-project settings
        await executionApi.updateSettings({
          maxConcurrentTasks: newSettings.execution.max_concurrent_tasks,
          projectIdeationMax: newSettings.execution.project_ideation_max,
          autoCommit: newSettings.execution.auto_commit,
          pauseOnFailure: newSettings.execution.pause_on_failure,
        }, currentProjectId || undefined);
      } catch (err) {
        console.error("Failed to save execution settings:", err);
        setSettingsError(err instanceof Error ? err.message : "Failed to save settings");
      } finally {
        setIsSavingSettings(false);
      }
    }, 300);
  }, [currentProjectId]);

  // Cleanup timeout on unmount
  useEffect(() => {
    return () => {
      if (saveTimeoutRef.current) {
        clearTimeout(saveTimeoutRef.current);
      }
    };
  }, []);

  // Phase 82: Pass currentProjectId to execution API calls for per-project scoping
  const handlePauseToggle = async () => {
    const isStopped = executionStatus.haltMode === "stopped";
    setIsExecutionLoading(true);
    try {
      const response = executionStatus.isPaused || isStopped
        ? await api.execution.resume(currentProjectId || undefined)
        : await api.execution.pause(currentProjectId || undefined);
      setExecutionStatus(response.status);
    } catch {
      toast.error(
        executionStatus.isPaused
          ? "Failed to resume execution"
          : isStopped
          ? "Failed to start execution"
          : "Failed to pause execution"
      );
    } finally {
      setIsExecutionLoading(false);
    }
  };

  const handleStop = async () => {
    setIsExecutionLoading(true);
    try {
      const response = await api.execution.stop(currentProjectId || undefined);
      setExecutionStatus(response.status);
    } catch {
      toast.error("Failed to stop execution");
    } finally {
      setIsExecutionLoading(false);
    }
  };

  const handlePauseProcess = async (taskId: string) => {
    try {
      await tasksApi.pause(taskId);
      toast.success("Task paused");
    } catch {
      toast.error("Failed to pause task");
    }
  };

  const handleStopProcess = async (taskId: string) => {
    try {
      await tasksApi.stop(taskId);
      toast.success("Task stopped");
    } catch {
      toast.error("Failed to stop task");
    }
  };

  const handleOpenSettings = () => {
    openModal("settings");
  };

  const handleBattleModeToggle = useCallback(() => {
    if (battleModeActive) {
      exitBattleMode();
      return;
    }
    enterBattleMode();
  }, [battleModeActive, enterBattleMode, exitBattleMode]);

  useEffect(() => {
    if (currentView !== "graph" && battleModeActive) {
      exitBattleMode();
    }
  }, [battleModeActive, currentView, exitBattleMode]);

  // Ideation handlers
  const handleNewSession = useCallback(() => {
    // Clear active session to show StartSessionPanel with mode selector
    setActiveSession(null);
  }, [setActiveSession]);

  const handleArchiveSession = useCallback(async (sessionId: string) => {
    try {
      await archiveSession.mutateAsync(sessionId);
      // Clean up stores to free memory
      archiveSessionInStore(sessionId);
      clearMessages(`session:${sessionId}`);
      setActiveSession(null);
    } catch {
      toast.error("Failed to archive session");
    }
  }, [archiveSession, setActiveSession, archiveSessionInStore, clearMessages]);

  const handleSelectSession = useCallback(async (sessionId: string) => {
    // Find the session in allSessions and select it atomically
    const session = allSessions.find((s) => s.id === sessionId);
    if (session) {
      selectSession(session);
      return;
    }

    // Session not in store (e.g. archived) — fetch from backend
    try {
      const fetchedSession = await ideationApi.sessions.get(sessionId);
      if (fetchedSession) {
        selectSession(fetchedSession);
      } else {
        toast.error("Failed to open session");
      }
    } catch {
      toast.error("Failed to open session");
    }
  }, [allSessions, selectSession]);

  const handleNavigateToSession = useCallback(async (sessionId: string) => {
    setCurrentView("ideation");
    await handleSelectSession(sessionId);
  }, [setCurrentView, handleSelectSession]);

  const handleEditProposal = useCallback((proposalId: string) => {
    setEditingProposalId(proposalId);
  }, []);

  const handleViewProposal = useCallback((proposalId: string, enrichment: ProposalDetailEnrichment) => {
    setViewingProposalId(proposalId);
    setViewingEnrichment(enrichment);
  }, []);

  const handleNavigateToTaskFromSheet = useCallback((taskId: string) => {
    setCurrentView("kanban");
    setSelectedTaskId(taskId);
  }, [setCurrentView, setSelectedTaskId]);

  const handleSaveProposal = useCallback(
    async (proposalId: string, data: UpdateProposalInput) => {
      try {
        await updateProposal.mutateAsync({ proposalId, changes: data });
        setEditingProposalId(null);
        toast.success("Proposal updated");
      } catch {
        toast.error("Failed to update proposal");
      }
    },
    [updateProposal]
  );

  const handleRemoveProposal = useCallback((proposalId: string) => {
    deleteProposal.mutate(proposalId);
  }, [deleteProposal]);

  const handleReorderProposals = useCallback((proposalIds: string[]) => {
    if (activeSession) {
      reorder.mutate({ sessionId: activeSession.id, proposalIds });
    }
  }, [activeSession, reorder]);

  const handleApplyProposals = useCallback(async (options: ApplyProposalsInput) => {
    try {
      const result = await applyProposalsMutation.mutateAsync(options);
      if (result.sessionConverted) {
        const count = result.createdTaskIds.length;
        toast.success(`Plan accepted — ${count} ${count === 1 ? "task" : "tasks"} created`, {
          action: {
            label: "View Work",
            onClick: () => setCurrentView("graph"),
          },
          duration: 6000,
        });
      }
      return result;
    } catch (error) {
      toast.error(extractErrorMessage(error, "Failed to apply proposals"));
      throw error;
    }
  }, [applyProposalsMutation, setCurrentView]);

  // Project wizard handlers
  const handleOpenProjectWizard = useCallback(() => {
    setProjectCreationError(null);
    setIsProjectWizardOpen(true);
  }, []);

  const handleCloseProjectWizard = useCallback(() => {
    setIsProjectWizardOpen(false);
    setProjectCreationError(null);
  }, []);

  const handleCreateProject = useCallback(async (projectData: CreateProject) => {
    setIsCreatingProject(true);
    setProjectCreationError(null);
    try {
      // Call Tauri backend to create project
      const newProject = await api.projects.create(projectData);
      // Invalidate the projects query so the useEffect sync doesn't overwrite with stale data
      await queryClient.invalidateQueries({ queryKey: projectKeys.list() });
      addProject(newProject);
      selectProject(newProject.id);
      setIsProjectWizardOpen(false);
    } catch (error) {
      setProjectCreationError(error instanceof Error ? error.message : "Failed to create project");
    } finally {
      setIsCreatingProject(false);
    }
  }, [addProject, selectProject]);

  const handleBrowseFolder = useCallback(async (): Promise<string | null> => {
    try {
      const selected = await openDialog({
        directory: true,
        multiple: false,
        title: "Select Project Folder",
      });
      // selected is string | string[] | null for directories
      if (typeof selected === "string") {
        return selected;
      }
      return null;
    } catch {
      return null;
    }
  }, []);

  const handleFetchBranches = useCallback(async (workingDirectory: string): Promise<string[]> => {
    try {
      const branches = await getGitBranches(workingDirectory);
      return branches;
    } catch {
      return [];
    }
  }, []);

  const handleDetectDefaultBranch = useCallback(async (workingDirectory: string): Promise<string> => {
    // Use backend detection with fallback chain (origin/HEAD -> main -> master -> first branch)
    return getGitDefaultBranch(workingDirectory);
  }, []);

  // Handler for closing manually-opened welcome screen
  const handleCloseWelcomeOverlay = useCallback(() => {
    if (welcomeOverlayReturnView) {
      setCurrentView(welcomeOverlayReturnView);
    }
    closeWelcomeOverlay();
  }, [welcomeOverlayReturnView, setCurrentView, closeWelcomeOverlay]);

  // Handler for view changes - clears task selection to reset state
  const handleViewChange = useCallback((view: ViewType) => {
    // Close any open task detail panel when switching views
    setSelectedTaskId(null);
    if (view === "agents") {
      if (currentView === "agents") {
        setCurrentView(agentsReturnViewRef.current);
        return;
      }
      agentsReturnViewRef.current =
        currentView === "task_detail" || currentView === "team" ? "kanban" : currentView;
    }
    setCurrentView(view);
  }, [currentView, setSelectedTaskId, setCurrentView]);

  const handleOpenNewAgent = useCallback(() => {
    const nextProjectId = activeProjectId ?? fetchedProjects?.[0]?.id ?? null;
    if (nextProjectId) {
      setFocusedAgentProject(nextProjectId);
    }
    clearAgentSelection();
    if (currentView !== "agents") {
      agentsReturnViewRef.current =
        currentView === "task_detail" || currentView === "team" ? "kanban" : currentView;
      setSelectedTaskId(null);
      setCurrentView("agents");
    }
  }, [
    activeProjectId,
    clearAgentSelection,
    currentView,
    fetchedProjects,
    setFocusedAgentProject,
    setSelectedTaskId,
    setCurrentView,
  ]);

  // Keyboard shortcuts for view switching, reviews toggle, and project creation
  const handleToggleGraphRightPanel = useCallback(() => {
    if (isNavCompact) {
      toggleGraphRightPanelCompactOpen();
    } else {
      toggleGraphRightPanelUserOpen();
    }
  }, [isNavCompact, toggleGraphRightPanelCompactOpen, toggleGraphRightPanelUserOpen]);

  const handleOpenPlanQuickSwitcher = useCallback(
    (source: SelectionSource = "quick_switcher") => {
      setPlanQuickSwitcherSource(source);
      setIsPlanQuickSwitcherOpen(true);
    },
    []
  );

  useAppKeyboardShortcuts({
    currentView,
    setCurrentView: handleViewChange,
    toggleReviewsPanel,
    toggleGraphRightPanel: handleToggleGraphRightPanel,
    openProjectWizard: handleOpenProjectWizard,
    hasProjects: !hasNoProjects,
    showWelcomeOverlay,
    openWelcomeOverlay,
    closeWelcomeOverlay,
    welcomeOverlayReturnView,
    openPlanQuickSwitcher: handleOpenPlanQuickSwitcher,
    onBattleModeToggle: handleBattleModeToggle,
    openSettings: handleOpenSettings,
    openNewAgent: handleOpenNewAgent,
    featureFlags,
  });

  // Global click handler to close quick switcher when clicking outside
  useEffect(() => {
    if (!isPlanQuickSwitcherOpen) return;

    const handleClickOutside = (e: MouseEvent) => {
      // Check if click is outside the quick switcher panel
      const target = e.target as HTMLElement;
      const quickSwitcherPanel = target.closest('[data-quick-switcher-panel]');

      if (!quickSwitcherPanel) {
        setIsPlanQuickSwitcherOpen(false);
      }
    };

    // Use capture phase to handle clicks before they bubble
    document.addEventListener('click', handleClickOutside, true);
    return () => document.removeEventListener('click', handleClickOutside, true);
  }, [isPlanQuickSwitcherOpen]);

  // Test page routing - return early if on a test page
  if (testPage) {
    return testPage;
  }

  const toastBottomOffset = (currentView === "kanban" || currentView === "graph") ? "92px" : "16px";
  const quickSwitcherAnchorSelector =
    currentView === "kanban"
      ? '[data-testid="kanban-split-left"]'
      : currentView === "graph"
        ? '[data-testid="graph-split-left"]'
        : undefined;

  return (
    <TooltipProvider delayDuration={300}>
      <main
        className="h-screen flex flex-col overflow-hidden"
        style={{ backgroundColor: "var(--app-content-bg)", color: "var(--text-primary)" }}
      >
      {/* Update checker - runs on mount, shows toast if update available */}
      <UpdateChecker />

      <AppTopBar
        currentView={currentView}
        pendingReviewCount={pendingReviewCount}
        reviewsPanelOpen={reviewsPanelOpen}
        onToggleReviewsPanel={toggleReviewsPanel}
      />

      {/* Spacer for fixed header */}
      <div className="h-12 flex-shrink-0" />

      {/* App body: left nav rail + main content */}
      <div className="flex-1 flex overflow-hidden" style={{ backgroundColor: "var(--app-content-bg)" }}>
        <LeftNavRail
          currentView={currentView}
          onViewChange={handleViewChange}
          onOpenSettings={handleOpenSettings}
          hideViews={hasNoProjects || showWelcomeOverlay}
        />

      {/* Main content area - shows WelcomeScreen or normal content */}
      {(hasNoProjects || showWelcomeOverlay) ? (
        /* Empty state or manual overlay: animated welcome screen */
        <WelcomeScreen
          onCreateProject={handleOpenProjectWizard}
          onClose={showWelcomeOverlay ? handleCloseWelcomeOverlay : undefined}
        />
      ) : (
        /* Normal content with view-specific content and optional panels */
        <div className="flex-1 flex overflow-hidden" style={{ backgroundColor: "var(--app-content-bg)" }}>
          {/* Main view area */}
          <div className="flex-1 flex flex-col overflow-hidden" style={{ backgroundColor: "var(--app-content-bg)" }}>
            <div className="flex-1 overflow-auto h-full" style={{ backgroundColor: "var(--app-content-bg)" }}>
              {currentView === "kanban" && (
                <KanbanSplitLayout
                  projectId={currentProjectId}
                  footer={
                    <ExecutionControlBar
                      projectId={currentProjectId}
                      runningCount={executionStatus.runningCount}
                      maxConcurrent={executionStatus.maxConcurrent}
                      queuedCount={executionStatus.queuedCount}
                      queuedMessageCount={executionStatus.queuedMessageCount ?? 0}
                      pausedCount={pausedCount}
                      pausedTasks={pausedTasks}
                      ideationActive={executionStatus.ideationActive}
                      ideationMax={executionStatus.ideationMaxProject}
                      ideationWaiting={executionStatus.ideationWaiting}
                      mergingCount={mergingCount}
                      mergeAttentionCount={mergeAttentionCount}
                      hasAttentionMerges={hasAttentionMerges}
                      mergePipelineData={mergePipelineData ?? null}
                      isPaused={executionStatus.isPaused}
                      haltMode={executionStatus.haltMode}
                      isLoading={isExecutionLoading}
                      onPauseToggle={handlePauseToggle}
                      onStop={handleStop}
                      runningProcesses={runningProcessesData?.processes ?? []}
                      ideationSessions={runningProcessesData?.ideationSessions ?? []}
                      onPauseProcess={handlePauseProcess}
                      onStopProcess={handleStopProcess}
                      onOpenSettings={handleOpenSettings}
                      onNavigateToSession={handleNavigateToSession}
                    />
                  }
                >
                  <TaskBoard
                    projectId={currentProjectId}
                    onOpenPlanQuickSwitcher={handleOpenPlanQuickSwitcher}
                  />
                </KanbanSplitLayout>
              )}
              {currentView === "graph" && (
                <TaskGraphView
                  projectId={currentProjectId}
                  onOpenPlanQuickSwitcher={handleOpenPlanQuickSwitcher}
                  footer={
                    <ExecutionControlBar
                      projectId={currentProjectId}
                      runningCount={executionStatus.runningCount}
                      maxConcurrent={executionStatus.maxConcurrent}
                      queuedCount={executionStatus.queuedCount}
                      queuedMessageCount={executionStatus.queuedMessageCount ?? 0}
                      pausedCount={pausedCount}
                      pausedTasks={pausedTasks}
                      ideationActive={executionStatus.ideationActive}
                      ideationMax={executionStatus.ideationMaxProject}
                      ideationWaiting={executionStatus.ideationWaiting}
                      mergingCount={mergingCount}
                      mergeAttentionCount={mergeAttentionCount}
                      hasAttentionMerges={hasAttentionMerges}
                      mergePipelineData={mergePipelineData ?? null}
                      isPaused={executionStatus.isPaused}
                      haltMode={executionStatus.haltMode}
                      isLoading={isExecutionLoading}
                      onPauseToggle={handlePauseToggle}
                      onStop={handleStop}
                      runningProcesses={runningProcessesData?.processes ?? []}
                      ideationSessions={runningProcessesData?.ideationSessions ?? []}
                      onPauseProcess={handlePauseProcess}
                      onStopProcess={handleStopProcess}
                      onOpenSettings={handleOpenSettings}
                      onNavigateToSession={handleNavigateToSession}
                    />
                  }
                />
              )}
              {currentView === "ideation" && (
                <IdeationView
                  session={resolvedSession}
                  proposals={proposals}
                  isSessionLoading={isSessionLoading}
                  onNewSession={handleNewSession}
                  onSelectSession={handleSelectSession}
                  onArchiveSession={handleArchiveSession}
                  onEditProposal={handleEditProposal}
                  onViewProposal={handleViewProposal}
                  selectedProposalId={viewingProposalId}
                  onRemoveProposal={handleRemoveProposal}
                  onReorderProposals={handleReorderProposals}
                  onApply={handleApplyProposals}
                  footer={
                    <ExecutionControlBar
                      projectId={currentProjectId}
                      runningCount={executionStatus.runningCount}
                      maxConcurrent={executionStatus.maxConcurrent}
                      queuedCount={executionStatus.queuedCount}
                      queuedMessageCount={executionStatus.queuedMessageCount ?? 0}
                      pausedCount={pausedCount}
                      pausedTasks={pausedTasks}
                      ideationActive={executionStatus.ideationActive}
                      ideationMax={executionStatus.ideationMaxProject}
                      ideationWaiting={executionStatus.ideationWaiting}
                      mergingCount={mergingCount}
                      mergeAttentionCount={mergeAttentionCount}
                      hasAttentionMerges={hasAttentionMerges}
                      mergePipelineData={mergePipelineData ?? null}
                      isPaused={executionStatus.isPaused}
                      haltMode={executionStatus.haltMode}
                      isLoading={isExecutionLoading}
                      onPauseToggle={handlePauseToggle}
                      onStop={handleStop}
                      runningProcesses={runningProcessesData?.processes ?? []}
                      ideationSessions={runningProcessesData?.ideationSessions ?? []}
                      onPauseProcess={handlePauseProcess}
                      onStopProcess={handleStopProcess}
                      onOpenSettings={handleOpenSettings}
                      onNavigateToSession={handleNavigateToSession}
                    />
                  }
                />
              )}
              {currentView === "agents" && (
                <AgentsView
                  projectId={currentProjectId}
                  onCreateProject={handleOpenProjectWizard}
                />
              )}
              {currentView === "extensibility" && (
                isViewEnabled("extensibility", featureFlags)
                  ? <ExtensibilityView />
                  : import.meta.env.DEV
                    ? <FeatureDisabledPlaceholder view="extensibility" yamlKey="extensibility_page" envVar="RALPHX_UI_EXTENSIBILITY_PAGE" />
                    : null
              )}
              {currentView === "activity" && (
                isViewEnabled("activity", featureFlags)
                  ? (
                    <ActivityView
                      showHeader
                      {...(activityFilter.taskId && { taskId: activityFilter.taskId })}
                      {...(activityFilter.sessionId && { sessionId: activityFilter.sessionId })}
                    />
                  )
                  : import.meta.env.DEV
                    ? <FeatureDisabledPlaceholder view="activity" yamlKey="activity_page" envVar="RALPHX_UI_ACTIVITY_PAGE" />
                    : null
              )}
              {currentView === "insights" && <InsightsView />}
              {currentView === "team" && <TeamSplitView />}
            </div>
        </div>

          {/* ReviewsPanel - right sidebar surface.
              bottomOffset 76 when ExecutionControlBar is visible below this
              panel (kanban/graph/ideation), 0 elsewhere so the panel fills
              the viewport instead of leaving a ~84px void. */}
          {reviewsPanelOpen && (
            <div
              className="fixed top-12 right-0 z-50 flex w-[400px] flex-col border-l"
              data-testid="reviews-panel-shell"
              style={{
                bottom: (currentView === "kanban" || currentView === "graph" || currentView === "ideation") ? "76px" : "0px",
                backgroundColor: "var(--app-sidebar-bg)",
                borderLeftColor: "var(--app-sidebar-border)",
                borderLeftStyle: "solid",
                borderLeftWidth: "1px",
              }}
            >
              <div
                className="flex flex-1 flex-col overflow-hidden"
                data-testid="reviews-panel-frame"
                style={{
                  backgroundColor: "var(--app-sidebar-bg)",
                  boxShadow: "none",
                }}
              >
                <ReviewsPanel
                  projectId={currentProjectId}
                  onClose={() => setReviewsPanelOpen(false)}
                  isApproving={isApproving}
                  isRequestingChanges={isRequestingChanges}
                />
              </div>
            </div>
          )}

        </div>
      )}
      </div>

      {/* Project Creation Wizard */}
      <ProjectCreationWizard
        isOpen={isProjectWizardOpen}
        onClose={handleCloseProjectWizard}
        onCreate={handleCreateProject}
        onBrowseFolder={handleBrowseFolder}
        onFetchBranches={handleFetchBranches}
        onDetectDefaultBranch={handleDetectDefaultBranch}
        isCreating={isCreatingProject}
        error={projectCreationError}
        isFirstRun={hasNoProjects}
      />

      {/* Settings Dialog - Modal overlay replacing routed settings view */}
      <SettingsDialog
        executionSettings={executionSettings}
        isLoadingSettings={isLoadingSettings}
        isSavingSettings={isSavingSettings}
        settingsError={settingsError}
        onSettingsChange={handleSettingsChange}
      />

      {/* Permission Dialog - Global UI-based permission approval */}
      <PermissionDialog />

      {/* Finalize Confirmation Dialog - Agent-initiated plan acceptance gate */}
      <FinalizeConfirmationDialog />

      {/* Verification Confirm Dialog - Agent/user-initiated verification gate with specialist selection */}
      <VerificationConfirmDialog />

      {/* Proposal Edit Modal - Edit ideation proposals */}
      <ProposalEditModal
        proposal={editingProposal}
        onSave={handleSaveProposal}
        onCancel={() => setEditingProposalId(null)}
        isSaving={updateProposal.isPending}
      />

      {/* Proposal Detail Sheet - Read-only detail view */}
      <ProposalDetailSheet
        proposal={viewingProposal}
        {...(viewingEnrichment !== undefined && { enrichment: viewingEnrichment })}
        isReadOnly={isIdeationReadOnly}
        onClose={() => { setViewingProposalId(null); setViewingEnrichment(undefined); }}
        onEdit={handleEditProposal}
        onDelete={handleRemoveProposal}
        onNavigateToTask={handleNavigateToTaskFromSheet}
      />

      {/* Plan Quick Switcher */}
      {!hasNoProjects && (
        <PlanQuickSwitcherPalette
          projectId={currentProjectId}
          isOpen={isPlanQuickSwitcherOpen}
          onClose={() => setIsPlanQuickSwitcherOpen(false)}
          selectionSource={planQuickSwitcherSource}
          {...(quickSwitcherAnchorSelector
            ? { anchorSelector: quickSwitcherAnchorSelector }
            : {})}
        />
      )}

      {/* Toast notifications */}
      <Toaster position="bottom-left" offset={toastBottomOffset} />
      </main>
    </TooltipProvider>
  );
}

function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <EventProvider>
        <AppContent />
      </EventProvider>
    </QueryClientProvider>
  );
}

export default App;
