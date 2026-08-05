/**
 * RalphX - App Shell
 * Root component with QueryClientProvider and EventProvider
 */

import { Suspense, useMemo, useState, useEffect, useCallback, useRef } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useShallow } from "zustand/react/shallow";
import { toast } from "sonner";
import { lazyWithRetry } from "@/lib/lazy-with-retry";
import { EnvironmentScopedProviders } from "@/providers/EnvironmentScopedProviders";
import { RemoteConnectionBanner } from "@/components/remote/RemoteConnectionBanner";
import { NotificationCenterPanel } from "@/components/notifications/NotificationCenterPanel";
import { ExecutionControlBar } from "@/components/execution/ExecutionControlBar";
import {
  resolveExecutionTaskAgentWorkspace,
  type ExecutionBarTaskNavigationTarget,
} from "@/components/execution/executionTaskNavigation";
import { requestAutomationRunOpen } from "@/components/automations/automationRunNavigation";
import { AppTopBar, LeftNavRail } from "@/components/layout";
import { PermissionDialog } from "@/components/PermissionDialog";
import { FinalizeConfirmationDialog } from "@/components/Ideation";
import { ExtensibilityView } from "@/components/ExtensibilityView";
import { ActivityView } from "@/components/activity";
import { GitHubBranchesView, githubBranchOverviewKeys } from "@/components/github";
import {
  GranolaDashboardView,
  granolaComposerReference,
  granolaDashboardKeys,
} from "@/components/granola";
import { TicketingDashboardView } from "@/components/ticketing";
import SettingsDialog from "@/components/settings/SettingsDialog";
import { InsightsView } from "@/components/views/InsightsView";
import { AgentIssueReportDialog } from "@/components/agents/AgentIssueReportDialog";
import { WelcomeScreen } from "@/components/WelcomeScreen";
import { StartupMaintenance } from "@/components/StartupMaintenance";
import { ProjectCreationWizard } from "@/components/projects/ProjectCreationWizard";
import { useUiStore } from "@/stores/uiStore";
import { useTaskStore, selectTasksByStatus } from "@/stores/taskStore";
import { useChatStore } from "@/stores/chatStore";
import { useProjectStore } from "@/stores/projectStore";
import { useAgentSessionStore } from "@/stores/agentSessionStore";
import { useIntegrationDashboardStore } from "@/stores/integrationDashboardStore";
import { DEFAULT_APP_VIEW, type AppView } from "@/types/app-view";
import { useTicketingStore } from "@/stores/ticketingStore";
import type { CreateProject } from "@/types/project";
import { useAttentionItems } from "@/hooks/useAttentionItems";
import { useUnreadNotificationCount } from "@/hooks/useNotificationHistory";
import { useExecutionEvents } from "@/hooks/useExecutionEvents";
import { useExecutionStatus } from "@/hooks/useExecutionControl";
import { useRunningProcesses } from "@/hooks/useRunningProcesses";
import { useMergePipeline } from "@/hooks/useMergePipeline";
import { useProjects, projectKeys } from "@/hooks/useProjects";
import { useAppKeyboardShortcuts } from "@/hooks/useAppKeyboardShortcuts";
import { useFeatureFlags, isViewEnabled } from "@/hooks/useFeatureFlags";
import { useIsRemoteEnvironment } from "@/hooks/useActiveEnvironment";
import { useHarnessProviders } from "@/hooks/useHarnessProviders";
import { useRemoteProviderReadiness } from "@/hooks/useRemoteProviderReadiness";
import { useTicketingCacheEvents } from "@/hooks/useTicketingEvents";
import { useAutomationEvents } from "@/hooks/useAutomations";
import { cn } from "@/lib/utils";
import { initializeEnvironmentRuntime } from "@/lib/remote/environment-runtime";
import {
  openTaskInAgents,
  navigateToIdeationSession,
} from "@/lib/navigation";
import { readFreshPostUpdatePreparingMarker } from "@/lib/postUpdatePreparing";
import type { StartupStatus } from "@/api/startup";
import { api, getGitBranches, getGitDefaultBranch } from "@/lib/tauri";
import { executionApi } from "@/api/execution";
import type { RunningWorkspaceSession } from "@/api/running-processes";
import { githubApi } from "@/api/github";
import { granolaApi, type GranolaNoteDetail, type GranolaNoteSummary } from "@/api/granola";
import { tasksApi } from "@/api/tasks";
import { ticketingApi, type TicketDeepLink } from "@/api/ticketing";
import { ticketingKeys } from "@/hooks/useTicketing";
import type { ProjectSettings } from "@/types/settings";
import { DEFAULT_PROJECT_SETTINGS } from "@/types/settings";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { TooltipProvider } from "@/components/ui/tooltip";
import { Toaster } from "@/components/ui/sonner";
import { ScreenshotGalleryTestPage } from "@/test-pages/ScreenshotGalleryTest";
import { ChatActivityVisualTestPage } from "@/test-pages/ChatActivityVisualTest";
import { preloadAutomationsView } from "@/components/automations/preloadAutomationsView";

const ATLASSIAN_AWARENESS_TOAST_KEY = "ralphx.atlassianIntegrationAwareness.v1";
const LazyAutomationsView = lazyWithRetry(() => preloadAutomationsView());
const LazyAgentsView = lazyWithRetry(async () => {
  const { AgentsView } = await import("@/components/agents/AgentsView");
  return { default: AgentsView };
});

function ensureCreatedProjectVisibleInAgentFilters(projectId: string) {
  const {
    showAllProjects,
    sidebarProjectFilterIds,
    setSidebarProjectFilterIds,
  } = useAgentSessionStore.getState();
  if (!showAllProjects) {
    setSidebarProjectFilterIds([
      ...new Set([...sidebarProjectFilterIds, projectId]),
    ]);
  }
}

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

  if (testPage === "chat-activity") {
    return <ChatActivityVisualTestPage />;
  }

  return null;
}

function FeatureDisabledPlaceholder({
  view,
  yamlKey,
  envVar,
  settingsPath,
}: {
  view: string;
  yamlKey?: string;
  envVar?: string;
  settingsPath?: string;
}) {
  return (
    <div
      className="flex flex-col items-center justify-center h-full gap-4 p-8 text-center"
      data-testid={`feature-disabled-${view}`}
    >
      <p className="text-sm font-semibold" style={{ color: "var(--text-primary)" }}>
        {view} page is disabled (dev mode)
      </p>
      <div className="text-xs font-mono rounded p-3 text-left" style={{ backgroundColor: "var(--bg-surface)", color: "var(--text-secondary)" }}>
        {settingsPath ? (
          <p className="font-sans" style={{ color: "var(--text-muted)" }}>
            Enable it in {settingsPath}.
          </p>
        ) : (
          <>
            <p className="mb-2 font-sans" style={{ color: "var(--text-muted)" }}>Enable via ralphx.yaml:</p>
            <pre>{`ui:\n  feature_flags:\n    ${yamlKey}: true`}</pre>
            <p className="mt-3 mb-1 font-sans" style={{ color: "var(--text-muted)" }}>Or via env var:</p>
            <pre>{`${envVar}=true`}</pre>
          </>
        )}
      </div>
    </div>
  );
}

function AutomationsRouteShell() {
  return (
    <div
      className="flex h-full min-h-0 flex-col"
      style={{ backgroundColor: "var(--app-content-bg)" }}
      data-testid="automations-view-shell"
    >
      <div
        className="flex items-center justify-between border-b px-6 py-5"
        style={{
          backgroundColor: "var(--app-content-bg)",
          borderBottomColor: "var(--border-default)",
          borderBottomStyle: "solid",
          borderBottomWidth: "1px",
        }}
      >
        <div>
          <div className="h-3 w-24 rounded" style={{ backgroundColor: "var(--bg-surface)" }} />
          <div className="mt-3 h-6 w-48 rounded" style={{ backgroundColor: "var(--bg-surface)" }} />
        </div>
        <div className="h-9 w-36 rounded-md" style={{ backgroundColor: "var(--bg-surface)" }} />
      </div>
      <div className="space-y-3 p-6">
        {[0, 1, 2].map((index) => (
          <div
            key={index}
            className="h-[72px] rounded-md"
            style={{
              backgroundColor: "var(--bg-surface)",
              borderColor: "var(--border-default)",
              borderStyle: "solid",
              borderWidth: "1px",
            }}
          />
        ))}
      </div>
    </div>
  );
}

function AgentsRouteShell() {
  return (
    <div
      className="flex h-full min-h-0 flex-col p-6"
      data-testid="agents-view-shell"
      style={{ backgroundColor: "var(--app-content-bg)" }}
    >
      <div className="h-4 w-36 rounded" style={{ backgroundColor: "var(--bg-surface)" }} />
      <div className="mt-4 h-24 rounded-md" style={{ backgroundColor: "var(--bg-surface)" }} />
    </div>
  );
}

function AppContent({ backgroundSettled }: { backgroundSettled: boolean }) {
  const queryClient = useQueryClient();
  // Check for test page first (must happen before any hooks for ESLint compliance)
  const testPage = useMemo(() => getTestPage(), []);

  const notificationsPanelOpen = useUiStore((s) => s.notificationsPanelOpen);
  const toggleNotificationsPanel = useUiStore((s) => s.toggleNotificationsPanel);
  const setNotificationsPanelOpen = useUiStore((s) => s.setNotificationsPanelOpen);
  const executionStatus = useUiStore((s) => s.executionStatus);
  const executionStatusKnown = useUiStore((s) => s.executionStatusKnown);
  const setExecutionStatus = useUiStore((s) => s.setExecutionStatus);
  const currentView = useUiStore((s) => s.currentView);
  const setCurrentView = useUiStore((s) => s.setCurrentView);
  const [selectedAutomationId, setSelectedAutomationId] = useState<string | null>(null);
  const activeModal = useUiStore((s) => s.activeModal);
  const openModal = useUiStore((s) => s.openModal);
  const { data: featureFlags } = useFeatureFlags();
  /**
   * Project creation is host-impossible from a remote client (2.6-a): the wizard's
   * folder picker (`openDialog`) reads THIS device's filesystem, so a path it returns
   * means nothing to the host. The affordances are hidden at their render sites
   * rather than guarded inside the handlers — a button that opens a wizard whose only
   * input cannot be supplied is not an honest control.
   */
  const isRemoteEnvironment = useIsRemoteEnvironment();
  const canCreateProjects = !isRemoteEnvironment;

  // Redirect to the default project view in production when the current view is disabled.
  // Ticketing remains directly reachable when a provider enables the dashboard
  // entry; provider availability is handled by the dashboard/sidebar surfaces.
  useEffect(() => {
    if (
      (isRemoteEnvironment && (currentView === "ticketing" || currentView === "github")) ||
      (currentView !== "ticketing" &&
        !import.meta.env.DEV &&
        !isViewEnabled(currentView, featureFlags))
    ) {
      setCurrentView(DEFAULT_APP_VIEW);
    }
  }, [currentView, featureFlags, isRemoteEnvironment, setCurrentView]);

  // Welcome screen overlay state
  const showWelcomeOverlay = useUiStore((s) => s.showWelcomeOverlay);
  const welcomeOverlayReturnView = useUiStore((s) => s.welcomeOverlayReturnView);
  const openWelcomeOverlay = useUiStore((s) => s.openWelcomeOverlay);
  const closeWelcomeOverlay = useUiStore((s) => s.closeWelcomeOverlay);
  // Activity filter state (for context-aware navigation from StatusActivityBadge)
  const activityFilter = useUiStore((s) => s.activityFilter);

  const switchToProject = useUiStore((s) => s.switchToProject);
  const preserveCurrentViewOnNextProjectSwitch = useUiStore(
    (s) => s.preserveCurrentViewOnNextProjectSwitch,
  );

  // Project state
  const activeProjectId = useProjectStore((s) => s.activeProjectId);
  const setProjects = useProjectStore((s) => s.setProjects);
  const addProject = useProjectStore((s) => s.addProject);
  const selectProject = useProjectStore((s) => s.selectProject);
  const clearAgentSelection = useAgentSessionStore((s) => s.clearSelection);
  const setFocusedAgentProject = useAgentSessionStore((s) => s.setFocusedProject);
  const {
    selectedAgentProjectId,
    selectedAgentConversationId,
    focusedAgentProjectId,
  } = useAgentSessionStore(
    useShallow((s) => ({
      selectedAgentProjectId: s.selectedProjectId,
      selectedAgentConversationId: s.selectedConversationId,
      focusedAgentProjectId: s.focusedProjectId,
    }))
  );

  const prevProjectIdRef = useRef<string | null>(null);
  const showsExecutionFooter = currentView === "agents";
  const shouldHydrateAgentHaltState = currentView === "agents";
  const shouldHydrateExecutionStatus =
    showsExecutionFooter || shouldHydrateAgentHaltState;
  const currentProjectId = activeProjectId ?? "";
  const agentFooterProjectId = selectedAgentConversationId
    ? selectedAgentProjectId ?? focusedAgentProjectId ?? currentProjectId
    : focusedAgentProjectId ?? selectedAgentProjectId ?? currentProjectId;
  const executionProjectId =
    currentView === "agents" ? agentFooterProjectId : currentProjectId;
  const executionProjectParam = executionProjectId || undefined;
  const shouldPollExecutionStatus = showsExecutionFooter && Boolean(executionProjectParam);
  const shouldHydrateExecutionSettings = activeModal === "settings";

  // Fetch projects from backend
  const {
    data: fetchedProjects,
    isLoading: isLoadingProjects,
    isError: isProjectsError,
  } = useProjects();
  const activeProject = useMemo(
    () => fetchedProjects?.find((project) => project.id === currentProjectId) ?? null,
    [currentProjectId, fetchedProjects],
  );
  const {
    settings: providerSettings,
    isLoading: isLoadingProviderSettings,
    isPlaceholderData: isPlaceholderProviderSettings,
  } = useHarnessProviders();
  // Only the remote gate consumes this; on the local environment the query stays disabled.
  const { data: remoteProviderReadiness } =
    useRemoteProviderReadiness(isRemoteEnvironment);

  // Project creation wizard state
  const [isProjectWizardOpen, setIsProjectWizardOpen] = useState(false);
  const [isCreatingProject, setIsCreatingProject] = useState(false);
  const [projectCreationError, setProjectCreationError] = useState<string | null>(null);
  const [isAgentIssueReportOpen, setIsAgentIssueReportOpen] = useState(false);


  const [isExecutionLoading, setIsExecutionLoading] = useState(false);

  // Execution settings state (persisted to database)
  const [executionSettings, setExecutionSettings] = useState<ProjectSettings | null>(null);

  // Running processes data for popover
  const { data: runningProcessesData } = useRunningProcesses(executionProjectParam, {
    enabled: showsExecutionFooter && Boolean(executionProjectParam),
  });
  const [isLoadingSettings, setIsLoadingSettings] = useState(false);
  const [isSavingSettings, setIsSavingSettings] = useState(false);
  const [settingsError, setSettingsError] = useState<string | null>(null);
  const saveTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Check if we should show the empty state (no projects)
  // Use TanStack Query data directly — the Zustand store sync via useEffect
  // can lag behind, causing a brief flash where store.projects is {} while
  // fetchedProjects already has data.
  // A FAILED read is not an empty workspace. Under a remote environment an unavailable or
  // erroring project read would otherwise render first-run onboarding over a populated host —
  // the shape that made a connected client look unconfigured. Emptiness must be a successful
  // answer, never the absence of one.
  const hasNoProjects =
    !isLoadingProjects &&
    !isProjectsError &&
    (!fetchedProjects || fetchedProjects.length === 0);
  // `get_agent_provider_settings` is Denied on the facade, so a remote environment answers the
  // onboarding question from the readiness projection instead. Fail closed the same way: an
  // unresolved or failed readiness read never asserts that setup is required.
  const providerSetupRequired = isRemoteEnvironment
    ? remoteProviderReadiness !== undefined &&
      !remoteProviderReadiness.onboardingComplete
    : !isLoadingProviderSettings &&
      !isPlaceholderProviderSettings &&
      providerSettings.requiresOnboarding;
  const agentIssueReportContext = useMemo(() => {
    if (
      currentView !== "agents" ||
      hasNoProjects ||
      showWelcomeOverlay ||
      providerSetupRequired ||
      !selectedAgentProjectId ||
      !selectedAgentConversationId
    ) {
      return null;
    }
    return {
      projectId: selectedAgentProjectId,
      conversationId: selectedAgentConversationId,
    };
  }, [
    currentView,
    hasNoProjects,
    providerSetupRequired,
    selectedAgentConversationId,
    selectedAgentProjectId,
    showWelcomeOverlay,
  ]);
  const shouldShowAtlassianAwarenessAfterUpdateRef = useRef(
    readFreshPostUpdatePreparingMarker() !== null,
  );

  const attentionItems = useAttentionItems();
  const attentionCount = attentionItems.data?.length ?? 0;
  const unreadNotificationCount = useUnreadNotificationCount();
  const hasUnreadNotificationHistory = (unreadNotificationCount.data ?? 0) > 0;
  const [notificationPanelWasOpened, setNotificationPanelWasOpened] =
    useState(false);
  const shouldRenderNotificationPanel =
    notificationPanelWasOpened || notificationsPanelOpen;

  useEffect(() => {
    if (notificationsPanelOpen) {
      setNotificationPanelWasOpened(true);
    }
  }, [notificationsPanelOpen]);

  const closeNotificationsPanel = useCallback(() => {
    setNotificationsPanelOpen(false);
  }, [setNotificationsPanelOpen]);

  // Real-time execution status updates via Tauri events
  useExecutionEvents(executionProjectParam);
  useTicketingCacheEvents();
  useAutomationEvents();
  // Fetch initial execution status and poll every 30s as fallback
  // Scope the Agents footer to the selected/focused agent project instead of the app's active project.
  useExecutionStatus(executionProjectParam, {
    enabled: shouldHydrateExecutionStatus && Boolean(executionProjectParam),
    refetchInterval: shouldPollExecutionStatus ? 30000 : false,
    refetchOnWindowFocus: shouldPollExecutionStatus,
    staleTime: shouldHydrateAgentHaltState ? 30_000 : 0,
  });

  // Merge pipeline data
  const { data: mergePipelineData } = useMergePipeline(executionProjectParam, {
    enabled: showsExecutionFooter && Boolean(executionProjectParam),
  });
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

  // Project switch: save and restore the per-project app view.
  // Runs AFTER the setActiveProject backend sync effect (order matters in React)
  useEffect(() => {
    const prevId = prevProjectIdRef.current;
    prevProjectIdRef.current = activeProjectId;

    if (prevId !== activeProjectId && activeProjectId) {
      // Atomic view state save/clean/restore
      switchToProject(prevId, activeProjectId);

    }
  }, [activeProjectId, switchToProject]);

  // Load execution settings from database when project changes
  useEffect(() => {
    if (!shouldHydrateExecutionSettings) {
      return;
    }

    let isCancelled = false;

    async function loadSettings() {
      try {
        setIsLoadingSettings(true);
        setSettingsError(null);
        // Phase 82: Pass currentProjectId for per-project settings
        const response = await executionApi.getSettings(currentProjectId || undefined);
        if (isCancelled) {
          return;
        }
        // Map API response (camelCase) to settings type (snake_case)
        setExecutionSettings({
          ...DEFAULT_PROJECT_SETTINGS,
          execution: {
            ...DEFAULT_PROJECT_SETTINGS.execution,
            max_concurrent_tasks: response.maxConcurrentTasks,
            project_ideation_max: response.projectIdeationMax,
            auto_commit: response.autoCommit,
            pause_on_failure: response.pauseOnFailure,
            agent_workspace_pr_autofix_default: response.agentWorkspacePrAutofixDefault,
            agent_workspace_pr_auto_merge_default:
              response.agentWorkspacePrAutoMergeDefault,
          },
        });
      } catch (err) {
        if (isCancelled) {
          return;
        }
        console.error("Failed to load execution settings:", err);
        setSettingsError(err instanceof Error ? err.message : "Failed to load settings");
        // Fall back to defaults
        setExecutionSettings(DEFAULT_PROJECT_SETTINGS);
      } finally {
        if (!isCancelled) {
          setIsLoadingSettings(false);
        }
      }
    }
    loadSettings();
    return () => {
      isCancelled = true;
    };
  }, [currentProjectId, shouldHydrateExecutionSettings]);

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
          agentWorkspacePrAutofixDefault:
            newSettings.execution.agent_workspace_pr_autofix_default,
          agentWorkspacePrAutoMergeDefault:
            newSettings.execution.agent_workspace_pr_auto_merge_default,
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
        ? await api.execution.resume(executionProjectParam)
        : await api.execution.pause(executionProjectParam);
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
      const response = await api.execution.stop(executionProjectParam);
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

  const handleOpenSettings = useCallback(() => {
    openModal("settings");
  }, [openModal]);

  const handleOpenAgentIssueReport = useCallback(() => {
    setIsAgentIssueReportOpen(true);
  }, []);

  useEffect(() => {
    if (!agentIssueReportContext && isAgentIssueReportOpen) {
      setIsAgentIssueReportOpen(false);
    }
  }, [agentIssueReportContext, isAgentIssueReportOpen]);

  const handleOpenProviderSettings = useCallback(() => {
    openModal("settings", { section: "providers" });
  }, [openModal]);

  const handleOpenIntegrationSettings = useCallback(() => {
    openModal("settings", { section: "integrations" });
  }, [openModal]);

  const handleWarmView = useCallback((view: AppView) => {
    if (!currentProjectId) {
      return;
    }
    if (view === "ticketing") {
      void queryClient.prefetchQuery({
        queryKey: ticketingKeys.providers(currentProjectId),
        queryFn: () => ticketingApi.listProviders({ projectId: currentProjectId }),
        staleTime: 60_000,
      }).catch(() => {
        // Warm-up failures are non-blocking; opening the view surfaces real state.
      });
      return;
    }
    if (view === "github") {
      void queryClient.prefetchQuery({
        queryKey: githubBranchOverviewKeys.project(currentProjectId),
        queryFn: () => githubApi.getBranchOverview({ projectId: currentProjectId }),
        staleTime: 15_000,
      }).catch(() => {
        // Warm-up failures are non-blocking; opening the view surfaces real state.
      });
      return;
    }
    if (view === "granola") {
      void queryClient.prefetchQuery({
        queryKey: granolaDashboardKeys.settings(),
        queryFn: () => granolaApi.getSettings(),
        staleTime: 30_000,
      }).catch(() => {
        // Warm-up failures are non-blocking; opening the view surfaces real state.
      });
    }
  }, [currentProjectId]);

  const handleNavigateFromTicketAssociation = useCallback((deepLink: TicketDeepLink) => {
    const targetProjectId = deepLink.projectId ?? currentProjectId;
    const switchProjectForDeepLink = (view: AppView) => {
      setCurrentView(view);
      if (targetProjectId && targetProjectId !== activeProjectId) {
        preserveCurrentViewOnNextProjectSwitch();
        selectProject(targetProjectId);
      }
    };

    if (deepLink.view === "kanban" || deepLink.view === "graph") {
      const taskMode = deepLink.view;
      void openTaskInAgents(deepLink.id, taskMode, {
        projectId: targetProjectId,
        ...(deepLink.conversationId
          ? { conversationId: deepLink.conversationId }
          : {}),
      });
      return;
    }
    if (deepLink.view === "ideation") {
      navigateToIdeationSession(deepLink.id);
      return;
    }
    if (deepLink.view === "github" && targetProjectId) {
      useIntegrationDashboardStore.getState().setGitHubState(targetProjectId, {
        associationFilter: "pull_requests",
        searchQuery: deepLink.id,
        selectedBranchName: deepLink.id || null,
      });
      switchProjectForDeepLink("github");
      return;
    }
    if (deepLink.view === "granola" && targetProjectId) {
      useIntegrationDashboardStore.getState().setGranolaState(targetProjectId, {
        query: "",
        noteFilter: "all",
        selectedNoteId: deepLink.id || null,
      });
      switchProjectForDeepLink("granola");
      return;
    }
    if (deepLink.view === "ticketing") {
      if (deepLink.id) {
        useTicketingStore.getState().setFilters({ text: deepLink.id });
      }
      switchProjectForDeepLink("ticketing");
      return;
    }
    if (deepLink.view === "agents" && deepLink.projectId) {
      // Select the exact linked conversation (not just switch to the Agents view)
      // so its linked ticket and artifact are visible on arrival.
      const projectId = deepLink.projectId;
      setFocusedAgentProject(projectId);
      useAgentSessionStore.getState().selectConversation(projectId, deepLink.id);
      useChatStore.getState().setActiveConversation(`project:${projectId}`, deepLink.id);
      setCurrentView("agents");
      return;
    }
    setCurrentView(deepLink.view);
  }, [
    activeProjectId,
    currentProjectId,
    preserveCurrentViewOnNextProjectSwitch,
    selectProject,
    setCurrentView,
    setFocusedAgentProject,
  ]);

  const handleStartConversationFromGranolaNote = useCallback((
    note: GranolaNoteDetail | GranolaNoteSummary,
    targetProjectId: string,
  ) => {
    useAgentSessionStore.getState().setStartConversationDraft({
      projectId: targetProjectId,
      content: "",
      mode: "edit",
      composerIntegrationReferences: [granolaComposerReference(note)],
    });
    setFocusedAgentProject(targetProjectId);
    clearAgentSelection();
    useChatStore.getState().setActiveConversation(`project:${targetProjectId}`, null);
    setCurrentView("agents");
  }, [clearAgentSelection, setCurrentView, setFocusedAgentProject]);

  useEffect(() => {
    if (
      !shouldShowAtlassianAwarenessAfterUpdateRef.current ||
      hasNoProjects ||
      providerSetupRequired
    ) {
      return;
    }
    if (window.localStorage.getItem(ATLASSIAN_AWARENESS_TOAST_KEY) === "seen") {
      shouldShowAtlassianAwarenessAfterUpdateRef.current = false;
      return;
    }
    shouldShowAtlassianAwarenessAfterUpdateRef.current = false;
    window.localStorage.setItem(ATLASSIAN_AWARENESS_TOAST_KEY, "seen");
    toast.info("Atlassian integrations are available", {
      description: "Connect Jira and Confluence from Settings.",
      action: {
        label: "Open Integrations",
        onClick: handleOpenIntegrationSettings,
      },
      duration: 10000,
    });
  }, [
    handleOpenIntegrationSettings,
    hasNoProjects,
    providerSetupRequired,
  ]);

  const handleNavigateToSession = useCallback((sessionId: string) => {
    navigateToIdeationSession(sessionId);
  }, []);

  const handleOpenAutomationDetail = useCallback((automationId: string) => {
    setSelectedAutomationId(automationId);
    setCurrentView("automations");
  }, [setCurrentView]);

  const handleNavigateToWorkspace = useCallback((
    projectId: string,
    conversationId: string,
    session?: RunningWorkspaceSession,
  ) => {
    if (session?.automationId && session.automationRunId) {
      void requestAutomationRunOpen(
        queryClient,
        {
          projectId,
          automationId: session.automationId,
          runId: session.automationRunId,
          conversationId,
        },
        {
          fallback: "detail",
          onOpenAutomationDetail: handleOpenAutomationDetail,
        },
      );
      return;
    }
    setFocusedAgentProject(projectId);
    useAgentSessionStore.getState().selectConversation(projectId, conversationId);
    useChatStore.getState().setActiveConversation(`project:${projectId}`, conversationId);
    setCurrentView("agents");
  }, [handleOpenAutomationDetail, setCurrentView, setFocusedAgentProject]);

  const handleNavigateToExecutionTask = useCallback(
    (target: ExecutionBarTaskNavigationTarget) => {
      const agentWorkspace = resolveExecutionTaskAgentWorkspace(target);
      if (!agentWorkspace) {
        toast.info("No Agent conversation is linked to this task yet");
        return;
      }
      void openTaskInAgents(target.taskId, "graph", {
        projectId: agentWorkspace.projectId,
        conversationId: agentWorkspace.conversationId,
      });
    },
    [],
  );

  const handleNavigateToAutomationRun = useCallback(
    (target: Parameters<typeof requestAutomationRunOpen>[1]) => {
      void requestAutomationRunOpen(
        queryClient,
        target,
        {
          fallback: "detail",
          onOpenAutomationDetail: handleOpenAutomationDetail,
        },
      );
    },
    [handleOpenAutomationDetail],
  );

  const handleNewAutomation = useCallback(() => {
    if (!currentProjectId) {
      return;
    }
    useAgentSessionStore.getState().setStartConversationDraft({
      projectId: currentProjectId,
      content: "",
      mode: "automation",
      automationAuthoringMode: "trusted_auto_finalize",
    });
    clearAgentSelection();
    setFocusedAgentProject(currentProjectId);
    useChatStore.getState().setActiveConversation(`project:${currentProjectId}`, null);
    setCurrentView("agents");
  }, [
    clearAgentSelection,
    currentProjectId,
    setCurrentView,
    setFocusedAgentProject,
  ]);

  useEffect(() => {
    setSelectedAutomationId(null);
  }, [currentProjectId]);

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
      setFocusedAgentProject(newProject.id);
      clearAgentSelection();
      ensureCreatedProjectVisibleInAgentFilters(newProject.id);
      setIsProjectWizardOpen(false);
    } catch (error) {
      setProjectCreationError(error instanceof Error ? error.message : "Failed to create project");
    } finally {
      setIsCreatingProject(false);
    }
  }, [
    addProject,
    clearAgentSelection,
    selectProject,
    setFocusedAgentProject,
  ]);

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
    return getGitBranches(workingDirectory);
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

  // Root view changes do not own task selection; Agents does.
  const handleViewChange = useCallback((view: AppView) => {
    setCurrentView(view);
  }, [setCurrentView]);

  const handleOpenNewAgent = useCallback(() => {
    const nextProjectId = activeProjectId ?? fetchedProjects?.[0]?.id ?? null;
    if (nextProjectId) {
      setFocusedAgentProject(nextProjectId);
    }
    clearAgentSelection();
    if (currentView !== "agents") {
      setCurrentView("agents");
    }
  }, [
    activeProjectId,
    clearAgentSelection,
    currentView,
    fetchedProjects,
    setFocusedAgentProject,
    setCurrentView,
  ]);

  useAppKeyboardShortcuts({
    currentView,
    setCurrentView: handleViewChange,
    toggleNotificationsPanel,
    openProjectWizard: handleOpenProjectWizard,
    hasProjects: !hasNoProjects,
    showWelcomeOverlay,
    openWelcomeOverlay,
    closeWelcomeOverlay,
    welcomeOverlayReturnView,
    openSettings: handleOpenSettings,
    openNewAgent: handleOpenNewAgent,
    featureFlags,
  });

  // Test page routing - return early if on a test page
  if (testPage) {
    return testPage;
  }

  const executionFooter = executionProjectId ? (
    <ExecutionControlBar
      projectId={executionProjectId}
      statusKnown={executionStatusKnown}
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
      workspaceSessions={runningProcessesData?.workspaceSessions ?? []}
      lanes={runningProcessesData?.lanes ?? []}
      capacity={runningProcessesData?.capacity ?? null}
      onPauseProcess={handlePauseProcess}
      onStopProcess={handleStopProcess}
      onOpenSettings={handleOpenSettings}
      onNavigateToSession={handleNavigateToSession}
      onNavigateToWorkspace={handleNavigateToWorkspace}
      onNavigateToTask={handleNavigateToExecutionTask}
    />
  ) : null;

  const toastOffset = {
    bottom: showsExecutionFooter ? "92px" : "16px",
    left: "16px",
  };
  return (
    <TooltipProvider delayDuration={300}>
      <main
        className="h-screen flex flex-col overflow-hidden"
        style={{ backgroundColor: "var(--app-content-bg)", color: "var(--text-primary)" }}
      >
      <StartupMaintenance backgroundSettled={backgroundSettled} />

          <AppTopBar
            currentView={currentView}
            attentionCount={attentionCount}
            unreadNotificationCount={unreadNotificationCount.data ?? 0}
            attentionCountStale={attentionItems.isError}
            notificationsPanelOpen={notificationsPanelOpen}
            onToggleNotificationsPanel={toggleNotificationsPanel}
            {...(canCreateProjects ? { onNewProject: handleOpenProjectWizard } : {})}
            onProjectSwitchIntent={preserveCurrentViewOnNextProjectSwitch}
            showProjectSelector={
              !hasNoProjects && !showWelcomeOverlay && !providerSetupRequired
            }
          />

          {/* Spacer for fixed header */}
          <div className="h-12 flex-shrink-0" />

          {/*
            Degraded-connection banner for the active REMOTE environment. Mounted once,
            directly under the fixed-header spacer so it pushes the app body down rather
            than overlaying it, and above the nav rail so it is never scrolled out of
            view while the connection is down. Renders null on local / connected /
            suspended / flag-off (2.7-a).
          */}
          <RemoteConnectionBanner />

          {/* App body: left nav rail + main content */}
          <div className="flex-1 flex overflow-hidden" style={{ backgroundColor: "var(--app-content-bg)" }}>
            <LeftNavRail
              currentView={currentView}
              onViewChange={handleViewChange}
              onViewWarmUp={handleWarmView}
              onOpenSettings={handleOpenSettings}
              {...(agentIssueReportContext
                ? { onOpenIssueReport: handleOpenAgentIssueReport }
                : {})}
              hideViews={hasNoProjects || showWelcomeOverlay || providerSetupRequired}
            />

      {/* Main content area - shows WelcomeScreen or normal content */}
      {(hasNoProjects || showWelcomeOverlay || providerSetupRequired) ? (
        /* Empty state or manual overlay: animated welcome screen */
        <WelcomeScreen
          onCreateProject={handleOpenProjectWizard}
          onSetupProviders={handleOpenProviderSettings}
          onSetupIntegrations={handleOpenIntegrationSettings}
          providerSetupRequired={providerSetupRequired}
          hasProjects={!hasNoProjects}
          onClose={showWelcomeOverlay && !providerSetupRequired ? handleCloseWelcomeOverlay : undefined}
        />
      ) : (
        /* Normal content with view-specific content and optional panels */
        <div className="flex-1 flex overflow-hidden" style={{ backgroundColor: "var(--app-content-bg)" }}>
          {/* Main view area */}
          <div className="flex-1 flex flex-col overflow-hidden" style={{ backgroundColor: "var(--app-content-bg)" }}>
            <div className="flex-1 overflow-auto h-full" style={{ backgroundColor: "var(--app-content-bg)" }}>
              {currentView === "agents" && (
                <Suspense fallback={<AgentsRouteShell />}>
                  <LazyAgentsView
                    footer={executionFooter}
                    projectId={currentProjectId}
                    onCreateProject={handleOpenProjectWizard}
                    onOpenAutomation={handleOpenAutomationDetail}
                  />
                </Suspense>
              )}
              {currentView === "automations" && (
                isViewEnabled("automations", featureFlags)
                  ? (
                    <Suspense fallback={<AutomationsRouteShell />}>
                      <LazyAutomationsView
                        projectId={currentProjectId || null}
                        projectName={activeProject?.name ?? null}
                        projectOptions={fetchedProjects ?? []}
                        onProjectChange={selectProject}
                        selectedAutomationId={selectedAutomationId}
                        onSelectedAutomationChange={setSelectedAutomationId}
                        onNewAutomation={handleNewAutomation}
                        onOpenRunConversation={handleNavigateToWorkspace}
                        onOpenAutomationRun={handleNavigateToAutomationRun}
                      />
                    </Suspense>
                  )
                  : import.meta.env.DEV
                    ? <FeatureDisabledPlaceholder view="automations" yamlKey="automations_page" envVar="RALPHX_UI_AUTOMATIONS_PAGE" />
                    : null
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
              {currentView === "ticketing" && (
                <TicketingDashboardView
                  projectId={currentProjectId}
                  onNavigateToAssociation={handleNavigateFromTicketAssociation}
                />
              )}
              {currentView === "github" && (
                <GitHubBranchesView
                  projectId={currentProjectId}
                  project={activeProject}
                  onNavigateToAssociation={handleNavigateFromTicketAssociation}
                />
              )}
              {currentView === "granola" && (
                <GranolaDashboardView
                  projectId={currentProjectId}
                  project={activeProject}
                  projects={fetchedProjects ?? []}
                  onStartConversation={handleStartConversationFromGranolaNote}
                  onNavigateToAssociation={handleNavigateFromTicketAssociation}
                />
              )}
              {currentView === "insights" && <InsightsView />}
            </div>
        </div>

          {/* Notification center keeps a visually hidden light frame after first open. */}
          {shouldRenderNotificationPanel && (
            <>
              {notificationsPanelOpen && (
                <button
                  type="button"
                  tabIndex={-1}
                  aria-label="Close notifications outside panel"
                  className="fixed inset-x-0 top-12 z-40 cursor-default border-0 bg-transparent p-0"
                  data-testid="notifications-panel-backdrop"
                  onClick={closeNotificationsPanel}
                  style={{
                    bottom: "0px",
                  }}
                />
              )}
              <div
                className={cn(
                  "fixed top-12 right-0 z-50 flex flex-col border-l",
                  !notificationsPanelOpen && "pointer-events-none invisible",
                )}
                data-testid="notifications-panel-shell"
                aria-hidden={!notificationsPanelOpen}
                style={{
                  bottom: "0px",
                  width: "100vw",
                  maxWidth: "400px",
                  backgroundColor: "var(--bg-surface)",
                  borderLeftColor: "var(--border-subtle)",
                  borderLeftStyle: "solid",
                  borderLeftWidth: "1px",
                }}
              >
                <div
                  className="flex flex-1 flex-col overflow-hidden"
                  data-testid="notifications-panel-frame"
                  style={{
                    backgroundColor: "var(--bg-surface)",
                    boxShadow: "none",
                  }}
                >
                  <NotificationCenterPanel
                    isOpen={notificationsPanelOpen}
                    onClose={closeNotificationsPanel}
                    onOpenAutomationDetail={handleOpenAutomationDetail}
                    hasUnreadHistory={hasUnreadNotificationHistory}
                  />
                </div>
              </div>
            </>
          )}

        </div>
      )}
        </div>

      {/* Project Creation Wizard — host-only (2.6-a) */}
      {canCreateProjects && (
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
      )}

      {/* Settings Dialog - Modal overlay replacing routed settings view */}
      <SettingsDialog
        executionSettings={executionSettings}
        isLoadingSettings={isLoadingSettings}
        isSavingSettings={isSavingSettings}
        settingsError={settingsError}
        onSettingsChange={handleSettingsChange}
      />

      <AgentIssueReportDialog
        open={isAgentIssueReportOpen}
        onOpenChange={setIsAgentIssueReportOpen}
        context={agentIssueReportContext}
      />

      {/* Permission Dialog - Global UI-based permission approval */}
      <PermissionDialog />

      {/* Finalize Confirmation Dialog - Agent-initiated plan acceptance gate */}
      <FinalizeConfirmationDialog />

      {/* Toast notifications */}
      <Toaster position="bottom-left" offset={toastOffset} />
      </main>
    </TooltipProvider>
  );
}

function App({ startupStatus }: { startupStatus?: StartupStatus }) {
  const backgroundSettled = startupStatus?.backgroundComplete ?? true;

  useEffect(() => initializeEnvironmentRuntime(), []);

  return (
    <EnvironmentScopedProviders>
      <AppContent backgroundSettled={backgroundSettled} />
    </EnvironmentScopedProviders>
  );
}

export default App;
