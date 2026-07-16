/**
 * UI store using Zustand with immer middleware
 *
 * Manages ephemeral UI state: sidebar visibility, modal state,
 * notifications, loading states, confirmation dialogs, and
 * active user questions from agents.
 */

import { create } from "zustand";
import { immer } from "zustand/middleware/immer";
import { enableMapSet } from "immer";
import { invoke } from "@tauri-apps/api/core";
import { featureFlagsSchema } from "@/types/feature-flags";
import type { FeatureFlags } from "@/types/feature-flags";
import { applyFeatureFlagOverrides, isViewEnabled } from "@/lib/featureFlags";
import type { AskUserQuestionPayload } from "@/types/ask-user-question";
import type { ExecutionStatusResponse } from "@/lib/tauri";
import type { RecoveryPromptEvent } from "@/types/events";
import {
  DEFAULT_PROJECT_VIEW,
  normalizeMainView,
  type ViewType,
} from "@/types/chat";
import type { TaskHistoryState } from "@/types/task-history";
import {
  loadCollapsedColumns,
  saveCollapsedColumns,
} from "@/components/tasks/TaskBoard/Column.utils";

// ============================================================================
// Show Merge Tasks Persistence
// ============================================================================

const SHOW_MERGE_TASKS_KEY = "ralphx-show-merge-tasks";
const KANBAN_CARD_DISPLAY_MODE_KEY = "ralphx-kanban-card-display-mode";
const AUTOMATION_RUNS_DENSITY_KEY = "ralphx-automation-runs-density";

export type KanbanCardDisplayMode = "default" | "mini";
export type AutomationRunsDensity = "comfortable" | "compact";

function loadShowMergeTasks(): boolean {
  try {
    const saved = localStorage.getItem(SHOW_MERGE_TASKS_KEY);
    if (saved !== null) {
      return JSON.parse(saved) as boolean;
    }
  } catch {
    /* ignore parse errors */
  }
  return true; // default: visible
}

function saveShowMergeTasks(show: boolean): void {
  try {
    localStorage.setItem(SHOW_MERGE_TASKS_KEY, JSON.stringify(show));
  } catch {
    /* ignore write errors */
  }
}

function loadKanbanCardDisplayMode(): KanbanCardDisplayMode {
  try {
    const saved = localStorage.getItem(KANBAN_CARD_DISPLAY_MODE_KEY);
    if (saved === "default" || saved === "mini") {
      return saved;
    }
  } catch {
    /* ignore read errors */
  }
  return "default";
}

function saveKanbanCardDisplayMode(mode: KanbanCardDisplayMode): void {
  try {
    localStorage.setItem(KANBAN_CARD_DISPLAY_MODE_KEY, mode);
  } catch {
    /* ignore write errors */
  }
}

function loadAutomationRunsDensity(): AutomationRunsDensity {
  try {
    const saved = localStorage.getItem(AUTOMATION_RUNS_DENSITY_KEY);
    if (saved === "comfortable" || saved === "compact") {
      return saved;
    }
  } catch {
    /* ignore read errors */
  }
  return "comfortable";
}

function saveAutomationRunsDensity(density: AutomationRunsDensity): void {
  try {
    localStorage.setItem(AUTOMATION_RUNS_DENSITY_KEY, density);
  } catch {
    /* ignore write errors */
  }
}

function saveAutomationRunsDensityAfterPaint(density: AutomationRunsDensity): void {
  if (typeof requestAnimationFrame === "function") {
    requestAnimationFrame(() => {
      requestAnimationFrame(() => saveAutomationRunsDensity(density));
    });
    return;
  }
  setTimeout(() => saveAutomationRunsDensity(density), 0);
}
import { useIdeationStore } from "@/stores/ideationStore";
import { useProjectStore } from "@/stores/projectStore";

enableMapSet();

export type GraphSelection =
  | { kind: "task"; id: string }
  | { kind: "planGroup"; id: string }
  | { kind: "tierGroup"; id: string }
  | { kind: "customGroup"; id: string };

export interface TaskCreationContext {
  projectId: string;
  defaultTitle?: string;
  ideationSessionId?: string;
  executionPlanId?: string;
}

function applyTaskSelection(
  state: { selectedTaskId: string | null; taskHistoryState: UiState["taskHistoryState"] },
  taskId: string | null
): void {
  state.selectedTaskId = taskId;
  // Clear history state when task is deselected
  if (taskId === null) {
    state.taskHistoryState = null;
  }
}

// ============================================================================
// Per-Project Route Persistence
// ============================================================================

const VIEW_BY_PROJECT_KEY = "ralphx-views-by-project";
const SESSION_BY_PROJECT_KEY = "ralphx-sessions-by-project";
const SELECTED_TASK_BY_PROJECT_KEY = "ralphx-selected-task-by-project";

function loadViewByProject(): Record<string, ViewType> {
  try {
    const stored = localStorage.getItem(VIEW_BY_PROJECT_KEY);
    if (!stored) return {};
    const parsed = JSON.parse(stored) as Record<string, ViewType>;
    const normalized = Object.fromEntries(
      Object.entries(parsed).map(([projectId, view]) => [
        projectId,
        normalizeMainView(view),
      ]),
    ) as Record<string, ViewType>;
    if (JSON.stringify(normalized) !== stored) {
      saveViewByProject(normalized);
    }
    return normalized;
  } catch {
    return {};
  }
}

function saveViewByProject(map: Record<string, ViewType>): void {
  try {
    localStorage.setItem(VIEW_BY_PROJECT_KEY, JSON.stringify(map));
  } catch {
    /* ignore write errors */
  }
}

function loadSessionByProject(): Record<string, string | null> {
  try {
    const stored = localStorage.getItem(SESSION_BY_PROJECT_KEY);
    return stored ? (JSON.parse(stored) as Record<string, string | null>) : {};
  } catch {
    return {};
  }
}

function saveSessionByProject(map: Record<string, string | null>): void {
  try {
    localStorage.setItem(SESSION_BY_PROJECT_KEY, JSON.stringify(map));
  } catch {
    /* ignore write errors */
  }
}

function loadSelectedTaskByProject(): Record<string, string | null> {
  try {
    const stored = localStorage.getItem(SELECTED_TASK_BY_PROJECT_KEY);
    return stored ? (JSON.parse(stored) as Record<string, string | null>) : {};
  } catch {
    return {};
  }
}

function saveSelectedTaskByProject(map: Record<string, string | null>): void {
  try {
    localStorage.setItem(SELECTED_TASK_BY_PROJECT_KEY, JSON.stringify(map));
  } catch {
    /* ignore write errors */
  }
}

// ============================================================================
// Feature Flags (cached for synchronous guard use in Zustand actions)
// ============================================================================

const DEFAULT_FEATURE_FLAGS: FeatureFlags = {
  activityPage: true,
  extensibilityPage: true,
  ideationPage: false,
  automationsPage: true,
  battleMode: true,
  teamMode: false,
  atlassianOauth: false,
  ticketingDashboard: false,
};

// ============================================================================
// Types
// ============================================================================

/** Modal types available in the application */
export type ModalType =
  | "task-create"
  | "project-settings"
  | "settings"
  | "ask-user-question"
  | null;

/** Notification severity levels */
export type NotificationType = "success" | "error" | "warning" | "info";

/** A notification to display to the user */
export interface Notification {
  id: string;
  type: NotificationType;
  message: string;
  title?: string;
  duration?: number;
}

/** Filter for activity view navigation from StatusActivityBadge */
export interface ActivityFilter {
  taskId: string | null;
  sessionId: string | null;
}

export type ExecutionBarPopoverKind =
  | "running"
  | "queued"
  | "paused"
  | "merge"
  | "terminals"
  | null;
export type ExecutionBarRunningTab = "running" | "workspaces" | "execution" | "ideation";

/** Confirmation dialog configuration */
export interface ConfirmationConfig {
  title: string;
  message: string;
  confirmText?: string;
  cancelText?: string;
  onConfirm: () => void;
  onCancel?: () => void;
}

// ============================================================================
// State Interface
// ============================================================================

interface UiState {
  /** Whether the sidebar is open */
  sidebarOpen: boolean;
  /** Whether the notification center is open */
  notificationsPanelOpen: boolean;
  /** Current main view (kanban, ideation, etc.) */
  currentView: ViewType;
  /** Currently active modal type, or null if none */
  activeModal: ModalType;
  /** Context data for the active modal */
  modalContext: Record<string, unknown> | undefined;
  /** Active notifications */
  notifications: Notification[];
  /** Loading states for different parts of the UI */
  loading: Record<string, boolean>;
  /** Active confirmation dialog */
  confirmation: ConfirmationConfig | null;
  /** Active questions from agents, keyed by sessionId */
  activeQuestions: Record<string, AskUserQuestionPayload>;
  /** Answered question summaries, keyed by sessionId */
  answeredQuestions: Record<string, string>;
  /** Active recovery prompt from backend */
  recoveryPrompt: RecoveryPromptEvent | null;
  /** Surface that currently owns the recovery prompt dialog */
  recoveryPromptSurface: "chat" | "task_detail" | null;
  /** Current execution status (pause state, running/queued counts) */
  executionStatus: ExecutionStatusResponse;
  /** Currently open execution bar popover, if any */
  executionBarOpenPopover: ExecutionBarPopoverKind;
  /** Last selected tab inside the Running execution bar popover */
  executionBarRunningTab: ExecutionBarRunningTab;
  /** Whether to show archived tasks on the board */
  showArchived: boolean;
  /** Whether to show merge tasks on the board */
  showMergeTasks: boolean;
  /** Current search query for the task board */
  boardSearchQuery: string | null;
  /** App-wide Kanban card density preference */
  kanbanCardDisplayMode: KanbanCardDisplayMode;
  /** Automation run timeline density (persisted to localStorage) */
  automationRunsDensity: AutomationRunsDensity;
  /** Whether a search request is in flight */
  isSearching: boolean;
  /** ID of selected task for split-screen overlay (kanban view only) */
  selectedTaskId: string | null;
  /** Active selection in the task graph (single selection across types) */
  graphSelection: GraphSelection | null;
  /** User toggle for graph right panel visibility */
  graphRightPanelUserOpen: boolean;
  /** Compact-mode toggle for graph right panel visibility */
  graphRightPanelCompactOpen: boolean;
  /** Whether Battle Mode is active in Graph view */
  battleModeActive: boolean;
  /** Snapshot of graph panel visibility before entering battle mode */
  battleModePanelRestoreState: { userOpen: boolean; compactOpen: boolean } | null;
  /** History state for time-travel feature - shared between TaskDetailOverlay and IntegratedChatPanel */
  taskHistoryState: TaskHistoryState | null;
  /** Task creation overlay context, or null if closed */
  taskCreationContext: TaskCreationContext | null;
  /** Whether the welcome screen is manually shown (vs. empty state) */
  showWelcomeOverlay: boolean;
  /** View to return to when closing manually-opened welcome screen */
  welcomeOverlayReturnView: ViewType | null;
  /** View to return to when leaving team split view */
  previousView: ViewType | null;
  /** One-shot flag for top-bar project switches that should keep the visible section. */
  preserveCurrentViewOnProjectSwitch: boolean;
  /** Filter for activity view navigation (set by StatusActivityBadge) */
  activityFilter: ActivityFilter;
  /** Set of collapsed column IDs (persisted to localStorage) */
  collapsedColumns: Set<string>;
  /** Per-project last view (persisted to localStorage) */
  viewByProject: Record<string, ViewType>;
  /** Per-project last ideation session ID (persisted to localStorage) */
  sessionByProject: Record<string, string | null>;
  /** Per-project selected task detail ID (persisted to localStorage) */
  selectedTaskByProject: Record<string, string | null>;
  /** Cached UI feature flags (fetched once at startup, defaults to all-enabled) */
  featureFlags: FeatureFlags;
  /** Queue of session IDs awaiting finalization confirmation (first = active dialog) */
  pendingConfirmationQueue: string[];
  /** Global auto-accept: bypass confirmation dialog for all sessions (in-memory, resets on restart) */
  autoAcceptPlans: boolean;
  /** Per-session auto-accept: bypass confirmation for specific sessions (in-memory, resets on restart) */
  autoAcceptSessions: Set<string>;
}

// ============================================================================
// Actions Interface
// ============================================================================

interface UiActions {
  /** Toggle sidebar visibility */
  toggleSidebar: () => void;
  /** Set sidebar visibility directly */
  setSidebarOpen: (open: boolean) => void;
  /** Toggle notification center visibility */
  toggleNotificationsPanel: () => void;
  /** Set notification center visibility directly */
  setNotificationsPanelOpen: (open: boolean) => void;
  /** Set the current main view */
  setCurrentView: (view: ViewType) => void;
  /** Open a modal with optional context */
  openModal: (type: ModalType, context?: Record<string, unknown>) => void;
  /** Close the current modal */
  closeModal: () => void;
  /** Add a notification */
  addNotification: (notification: Notification) => void;
  /** Remove a notification by ID */
  removeNotification: (id: string) => void;
  /** Clear all notifications */
  clearNotifications: () => void;
  /** Set loading state for a key */
  setLoading: (key: string, loading: boolean) => void;
  /** Show a confirmation dialog */
  showConfirmation: (config: ConfirmationConfig) => void;
  /** Hide the confirmation dialog */
  hideConfirmation: () => void;
  /** Set active question for a session */
  setActiveQuestion: (sessionId: string, question: AskUserQuestionPayload) => void;
  /** Clear active question for a session */
  clearActiveQuestion: (sessionId: string) => void;
  /** Dismiss question for a session (clears both question and answered state) */
  dismissQuestion: (sessionId: string) => void;
  /** Set answered summary for a session */
  setAnsweredQuestion: (sessionId: string, summary: string) => void;
  /** Clear answered summary for a session */
  clearAnsweredQuestion: (sessionId: string) => void;
  /** Set active recovery prompt */
  setRecoveryPrompt: (prompt: RecoveryPromptEvent) => void;
  /** Clear active recovery prompt */
  clearRecoveryPrompt: () => void;
  /** Set surface that owns the recovery prompt dialog */
  setRecoveryPromptSurface: (surface: "chat" | "task_detail" | null) => void;
  /** Update full execution status from backend */
  setExecutionStatus: (status: ExecutionStatusResponse) => void;
  /** Set just the paused state */
  setExecutionPaused: (isPaused: boolean) => void;
  /** Set running count */
  setExecutionRunningCount: (count: number) => void;
  /** Set queued count */
  setExecutionQueuedCount: (count: number, queuedMessageCount?: number) => void;
  /** Set the currently open execution bar popover */
  setExecutionBarOpenPopover: (popover: ExecutionBarPopoverKind) => void;
  /** Set the selected tab inside the Running execution bar popover */
  setExecutionBarRunningTab: (tab: ExecutionBarRunningTab) => void;
  /** Set whether to show archived tasks */
  setShowArchived: (show: boolean) => void;
  /** Set whether to show merge tasks */
  setShowMergeTasks: (show: boolean) => void;
  /** Set the board search query */
  setBoardSearchQuery: (query: string | null) => void;
  /** Set app-wide Kanban card density preference */
  setKanbanCardDisplayMode: (mode: KanbanCardDisplayMode) => void;
  /** Set app-wide automation run timeline density */
  setAutomationRunsDensity: (density: AutomationRunsDensity) => void;
  /** Set whether a search is in progress */
  setIsSearching: (searching: boolean) => void;
  /** Set selected task ID for split-screen overlay */
  setSelectedTaskId: (taskId: string | null) => void;
  /** Set active graph selection */
  setGraphSelection: (selection: GraphSelection | null) => void;
  /** Clear active graph selection */
  clearGraphSelection: () => void;
  /** Toggle graph right panel visibility */
  toggleGraphRightPanel: () => void;
  /** Set graph right panel visibility */
  setGraphRightPanelUserOpen: (open: boolean) => void;
  /** Toggle compact-mode graph right panel visibility */
  toggleGraphRightPanelCompactOpen: () => void;
  /** Set compact-mode graph right panel visibility */
  setGraphRightPanelCompactOpen: (open: boolean) => void;
  /** Enter battle mode and capture graph panel visibility state */
  enterBattleMode: () => void;
  /** Exit battle mode and restore graph panel visibility state */
  exitBattleMode: () => void;
  /** Set task history state for time-travel feature */
  setTaskHistoryState: (state: TaskHistoryState | null) => void;
  /** Open task creation overlay */
  openTaskCreation: (
    projectId: string,
    defaultTitle?: string,
    context?: Pick<TaskCreationContext, "ideationSessionId" | "executionPlanId">
  ) => void;
  /** Close task creation overlay */
  closeTaskCreation: () => void;
  /** Open welcome screen overlay, saving current view */
  openWelcomeOverlay: () => void;
  /** Close welcome screen overlay, restoring previous view */
  closeWelcomeOverlay: () => void;
  /** Set activity filter for context-aware navigation */
  setActivityFilter: (filter: Partial<ActivityFilter>) => void;
  /** Clear activity filter */
  clearActivityFilter: () => void;
  /** Set collapse state for a specific column */
  setColumnCollapsed: (columnId: string, collapsed: boolean) => void;
  /** Toggle collapse state for a specific column */
  toggleColumnCollapsed: (columnId: string) => void;
  /** Expand a specific column (shorthand for setColumnCollapsed(id, false)) */
  expandColumn: (columnId: string) => void;
  /** Replace the entire collapsed columns set */
  setCollapsedColumns: (columns: Set<string>) => void;
  /** Set the view to return to when leaving team split view */
  setPreviousView: (view: ViewType | null) => void;
  /** Preserve the current section for the next project switch. */
  preserveCurrentViewOnNextProjectSwitch: () => void;
  /** Atomically save old project state, restore new project state, clear ephemeral state */
  switchToProject: (oldProjectId: string | null, newProjectId: string) => void;
  /** Remove stale per-project route entries for a deleted project */
  cleanupProjectRoute: (projectId: string) => void;
  /** Update cached feature flags (called once on startup after Tauri command resolves) */
  setFeatureFlags: (flags: FeatureFlags) => void;
  /** Atomically navigate to kanban view and select the given task */
  navigateToTask: (taskId: string) => void;
  /** Enqueue a session ID for finalization confirmation (deduplicates) */
  enqueuePendingConfirmation: (sessionId: string) => void;
  /** Remove the first item from the confirmation queue (after dialog resolves) */
  dequeueConfirmation: () => void;
  /** Remove a specific session from the queue (e.g., on background auto-accept) */
  removeFromConfirmationQueue: (sessionId: string) => void;
  /** Set global auto-accept toggle */
  setAutoAcceptPlans: (value: boolean) => void;
  /** Add a session to per-session auto-accept set */
  addAutoAcceptSession: (sessionId: string) => void;
  /** Remove a session from per-session auto-accept set */
  removeAutoAcceptSession: (sessionId: string) => void;
}

// ============================================================================
// Store Implementation
// ============================================================================

export const useUiStore = create<UiState & UiActions>()(
  immer((set, get) => ({
    // Initial state
    sidebarOpen: true,
    notificationsPanelOpen: false,
    currentView: DEFAULT_PROJECT_VIEW,
    activeModal: null,
    modalContext: undefined,
    notifications: [],
    loading: {},
    confirmation: null,
    activeQuestions: {},
    answeredQuestions: {},
    recoveryPrompt: null,
    recoveryPromptSurface: null,
    executionStatus: {
      isPaused: false,
      haltMode: "running",
      runningCount: 0,
      maxConcurrent: 10,
      globalMaxConcurrent: 20,
      queuedCount: 0,
      queuedMessageCount: 0,
      canStartTask: true,
      ideationActive: 0,
      ideationIdle: 0,
      ideationWaiting: 0,
      ideationMaxProject: 5,
      ideationMaxGlobal: 10,
    },
    executionBarOpenPopover: null,
    executionBarRunningTab: "execution",
    showArchived: false,
    showMergeTasks: loadShowMergeTasks(),
    boardSearchQuery: null,
    kanbanCardDisplayMode: loadKanbanCardDisplayMode(),
    automationRunsDensity: loadAutomationRunsDensity(),
    isSearching: false,
    selectedTaskId: null,
    graphSelection: null,
    graphRightPanelUserOpen: true,
    graphRightPanelCompactOpen: false,
    battleModeActive: false,
    battleModePanelRestoreState: null,
    taskHistoryState: null,
    taskCreationContext: null,
    showWelcomeOverlay: false,
    welcomeOverlayReturnView: null,
    previousView: null,
    preserveCurrentViewOnProjectSwitch: false,
    activityFilter: { taskId: null, sessionId: null },
    collapsedColumns: loadCollapsedColumns(),
    viewByProject: loadViewByProject(),
    sessionByProject: loadSessionByProject(),
    selectedTaskByProject: loadSelectedTaskByProject(),
    featureFlags: DEFAULT_FEATURE_FLAGS,
    pendingConfirmationQueue: [],
    autoAcceptPlans: false,
    autoAcceptSessions: new Set<string>(),

    // Actions
    toggleSidebar: () =>
      set((state) => {
        state.sidebarOpen = !state.sidebarOpen;
      }),

    setSidebarOpen: (open) =>
      set((state) => {
        state.sidebarOpen = open;
      }),

    toggleNotificationsPanel: () =>
      set((state) => {
        state.notificationsPanelOpen = !state.notificationsPanelOpen;
      }),

    setNotificationsPanelOpen: (open) =>
      set((state) => {
        state.notificationsPanelOpen = open;
      }),

    setCurrentView: (view) =>
      set((state) => {
        const normalizedView = normalizeMainView(view);
        const safeView =
          normalizedView === "ticketing" || isViewEnabled(normalizedView, state.featureFlags)
            ? normalizedView
            : DEFAULT_PROJECT_VIEW;
        const projectId = useProjectStore.getState().activeProjectId;
        state.currentView = safeView;
        if (projectId) {
          state.viewByProject[projectId] = safeView;
          saveViewByProject(state.viewByProject);
        }
      }),

    openModal: (type, context) =>
      set((state) => {
        state.activeModal = type;
        state.modalContext = context;
      }),

    closeModal: () =>
      set((state) => {
        state.activeModal = null;
        state.modalContext = undefined;
      }),

    addNotification: (notification) =>
      set((state) => {
        state.notifications.push(notification);
      }),

    removeNotification: (id) =>
      set((state) => {
        state.notifications = state.notifications.filter((n) => n.id !== id);
      }),

    clearNotifications: () =>
      set((state) => {
        state.notifications = [];
      }),

    setLoading: (key, loading) =>
      set((state) => {
        state.loading[key] = loading;
      }),

    showConfirmation: (config) =>
      set((state) => {
        state.confirmation = config;
      }),

    hideConfirmation: () =>
      set((state) => {
        state.confirmation = null;
      }),

    setActiveQuestion: (sessionId, question) =>
      set((state) => {
        state.activeQuestions[sessionId] = question;
      }),

    clearActiveQuestion: (sessionId) =>
      set((state) => {
        delete state.activeQuestions[sessionId];
      }),

    dismissQuestion: (sessionId) =>
      set((state) => {
        delete state.activeQuestions[sessionId];
        delete state.answeredQuestions[sessionId];
      }),

    setAnsweredQuestion: (sessionId, summary) =>
      set((state) => {
        state.answeredQuestions[sessionId] = summary;
      }),

    clearAnsweredQuestion: (sessionId) =>
      set((state) => {
        delete state.answeredQuestions[sessionId];
      }),

    setRecoveryPrompt: (prompt) =>
      set((state) => {
        state.recoveryPrompt = prompt;
        state.recoveryPromptSurface = null;
      }),

    clearRecoveryPrompt: () =>
      set((state) => {
        state.recoveryPrompt = null;
        state.recoveryPromptSurface = null;
      }),

    setRecoveryPromptSurface: (surface) =>
      set((state) => {
        state.recoveryPromptSurface = surface;
      }),

    setExecutionStatus: (status) =>
      set((state) => {
        state.executionStatus = status;
      }),

    setExecutionPaused: (isPaused) =>
      set((state) => {
        state.executionStatus.isPaused = isPaused;
        state.executionStatus.haltMode = isPaused ? "paused" : "running";
      }),

    setExecutionRunningCount: (count) =>
      set((state) => {
        state.executionStatus.runningCount = count;
      }),

    setExecutionQueuedCount: (count, queuedMessageCount) =>
      set((state) => {
        state.executionStatus.queuedCount = count;
        if (queuedMessageCount !== undefined) {
          state.executionStatus.queuedMessageCount = queuedMessageCount;
        }
      }),

    setExecutionBarOpenPopover: (popover) =>
      set((state) => {
        state.executionBarOpenPopover = popover;
      }),

    setExecutionBarRunningTab: (tab) =>
      set((state) => {
        state.executionBarRunningTab = tab;
      }),

    setShowArchived: (show) =>
      set((state) => {
        state.showArchived = show;
      }),

    setShowMergeTasks: (show) =>
      set((state) => {
        state.showMergeTasks = show;
        saveShowMergeTasks(show);
      }),

    setBoardSearchQuery: (query) =>
      set((state) => {
        state.boardSearchQuery = query;
      }),

    setKanbanCardDisplayMode: (mode) =>
      set((state) => {
        state.kanbanCardDisplayMode = mode;
        saveKanbanCardDisplayMode(mode);
      }),

    setAutomationRunsDensity: (density) => {
      set((state) => {
        state.automationRunsDensity = density;
      });
      saveAutomationRunsDensityAfterPaint(density);
    },

    setIsSearching: (searching) =>
      set((state) => {
        state.isSearching = searching;
      }),

    setSelectedTaskId: (taskId) =>
      set((state) => {
        applyTaskSelection(state, taskId);
        if (taskId !== null) {
          state.graphSelection = { kind: "task", id: taskId };
        } else if (state.graphSelection?.kind === "task") {
          state.graphSelection = null;
        }
        const projectId = useProjectStore.getState().activeProjectId;
        if (projectId) {
          state.selectedTaskByProject[projectId] = taskId;
          saveSelectedTaskByProject(state.selectedTaskByProject);
        }
      }),

    setGraphSelection: (selection) =>
      set((state) => {
        state.graphSelection = selection;
      }),

    clearGraphSelection: () =>
      set((state) => {
        state.graphSelection = null;
      }),

    toggleGraphRightPanel: () =>
      set((state) => {
        state.graphRightPanelUserOpen = !state.graphRightPanelUserOpen;
      }),

    setGraphRightPanelUserOpen: (open) =>
      set((state) => {
        state.graphRightPanelUserOpen = open;
      }),

    toggleGraphRightPanelCompactOpen: () =>
      set((state) => {
        state.graphRightPanelCompactOpen = !state.graphRightPanelCompactOpen;
      }),

    setGraphRightPanelCompactOpen: (open) =>
      set((state) => {
        state.graphRightPanelCompactOpen = open;
      }),

    enterBattleMode: () =>
      set((state) => {
        if (state.battleModeActive) return;
        state.battleModePanelRestoreState = {
          userOpen: state.graphRightPanelUserOpen,
          compactOpen: state.graphRightPanelCompactOpen,
        };
        state.battleModeActive = true;
        state.graphRightPanelUserOpen = false;
        state.graphRightPanelCompactOpen = false;
      }),

    exitBattleMode: () =>
      set((state) => {
        if (!state.battleModeActive) return;
        state.battleModeActive = false;
        if (state.battleModePanelRestoreState) {
          state.graphRightPanelUserOpen = state.battleModePanelRestoreState.userOpen;
          state.graphRightPanelCompactOpen = state.battleModePanelRestoreState.compactOpen;
        }
        state.battleModePanelRestoreState = null;
      }),

    setTaskHistoryState: (historyState) =>
      set((state) => {
        state.taskHistoryState = historyState;
      }),

    openTaskCreation: (projectId, defaultTitle, context) =>
      set((state) => {
        state.taskCreationContext = {
          projectId,
          ...(defaultTitle !== undefined && { defaultTitle }),
          ...(context?.ideationSessionId && { ideationSessionId: context.ideationSessionId }),
          ...(context?.executionPlanId && { executionPlanId: context.executionPlanId }),
        };
      }),

    closeTaskCreation: () =>
      set((state) => {
        state.taskCreationContext = null;
      }),

    openWelcomeOverlay: () =>
      set((state) => {
        state.welcomeOverlayReturnView = state.currentView;
        state.showWelcomeOverlay = true;
      }),

    closeWelcomeOverlay: () =>
      set((state) => {
        state.showWelcomeOverlay = false;
      }),

    setActivityFilter: (filter) =>
      set((state) => {
        if (filter.taskId !== undefined) {
          state.activityFilter.taskId = filter.taskId;
        }
        if (filter.sessionId !== undefined) {
          state.activityFilter.sessionId = filter.sessionId;
        }
      }),

    clearActivityFilter: () =>
      set((state) => {
        state.activityFilter = { taskId: null, sessionId: null };
      }),

    setColumnCollapsed: (columnId, collapsed) =>
      set((state) => {
        if (collapsed) {
          state.collapsedColumns.add(columnId);
        } else {
          state.collapsedColumns.delete(columnId);
        }
        saveCollapsedColumns(state.collapsedColumns);
      }),

    toggleColumnCollapsed: (columnId) =>
      set((state) => {
        if (state.collapsedColumns.has(columnId)) {
          state.collapsedColumns.delete(columnId);
        } else {
          state.collapsedColumns.add(columnId);
        }
        saveCollapsedColumns(state.collapsedColumns);
      }),

    expandColumn: (columnId) =>
      set((state) => {
        state.collapsedColumns.delete(columnId);
        saveCollapsedColumns(state.collapsedColumns);
      }),

    setCollapsedColumns: (columns) =>
      set((state) => {
        state.collapsedColumns = columns;
        saveCollapsedColumns(state.collapsedColumns);
      }),

    setPreviousView: (view) =>
      set((state) => {
        state.previousView = view;
      }),

    preserveCurrentViewOnNextProjectSwitch: () =>
      set((state) => {
        state.preserveCurrentViewOnProjectSwitch = true;
      }),

    switchToProject: (oldProjectId, newProjectId) =>
      set((state) => {
        // SAVE phase — skip if oldProjectId is null (first load)
        if (oldProjectId) {
          state.viewByProject[oldProjectId] = normalizeMainView(state.currentView);
          state.sessionByProject[oldProjectId] = useIdeationStore.getState().activeSessionId;
          state.selectedTaskByProject[oldProjectId] = state.selectedTaskId;
        }

        const preserveCurrentView = state.preserveCurrentViewOnProjectSwitch;
        state.preserveCurrentViewOnProjectSwitch = false;

        // RESTORE phase — resolve view, fallback ephemeral views to the default project view
        let restoredView: ViewType = preserveCurrentView
          ? normalizeMainView(state.currentView)
          : state.viewByProject[newProjectId] ?? DEFAULT_PROJECT_VIEW;
        restoredView = normalizeMainView(restoredView);
        const restoredSelectedTaskId = state.selectedTaskByProject[newProjectId] ?? null;
        // Guard against stale localStorage values ("settings" was removed from ViewType)
        if ((restoredView as string) === "settings" || restoredView === "team") {
          restoredView = DEFAULT_PROJECT_VIEW;
        }
        if (restoredView === "task_detail") {
          restoredView = DEFAULT_PROJECT_VIEW;
        }
        // Feature flag guard: redirect disabled views to the default project view
        if (!isViewEnabled(restoredView, state.featureFlags)) {
          restoredView = DEFAULT_PROJECT_VIEW;
        }
        if (preserveCurrentView) {
          state.viewByProject[newProjectId] = restoredView;
        }

        // Persist updated maps
        saveViewByProject(state.viewByProject);
        saveSessionByProject(state.sessionByProject);
        saveSelectedTaskByProject(state.selectedTaskByProject);

        // CLEAN + RESTORE (atomic)
        state.currentView = restoredView;
        state.selectedTaskId = restoredSelectedTaskId;
        state.graphSelection = restoredSelectedTaskId
          ? { kind: "task", id: restoredSelectedTaskId }
          : null;
        state.taskHistoryState = null;
        state.boardSearchQuery = null;
        state.battleModeActive = false;
        state.battleModePanelRestoreState = null;
        state.activityFilter = { taskId: null, sessionId: null };
        state.graphRightPanelUserOpen = false;
        state.graphRightPanelCompactOpen = false;
      }),

    cleanupProjectRoute: (projectId) =>
      set((state) => {
        delete state.viewByProject[projectId];
        delete state.sessionByProject[projectId];
        delete state.selectedTaskByProject[projectId];
        saveViewByProject(state.viewByProject);
        saveSessionByProject(state.sessionByProject);
        saveSelectedTaskByProject(state.selectedTaskByProject);
      }),

    setFeatureFlags: (flags) =>
      set((state) => {
        state.featureFlags = flags;
        if (!isViewEnabled(state.currentView, flags)) {
          state.currentView = DEFAULT_PROJECT_VIEW;
          const projectId = useProjectStore.getState().activeProjectId;
          if (projectId) {
            state.viewByProject[projectId] = DEFAULT_PROJECT_VIEW;
            saveViewByProject(state.viewByProject);
          }
        }
      }),

    navigateToTask: (taskId) => {
      get().setCurrentView("agents");
      set((state) => {
        applyTaskSelection(state, taskId);
        state.graphSelection = { kind: "task", id: taskId };
        const projectId = useProjectStore.getState().activeProjectId;
        if (projectId) {
          state.selectedTaskByProject[projectId] = taskId;
          saveSelectedTaskByProject(state.selectedTaskByProject);
        }
      });
    },

    enqueuePendingConfirmation: (sessionId) =>
      set((state) => {
        if (!state.pendingConfirmationQueue.includes(sessionId)) {
          state.pendingConfirmationQueue.push(sessionId);
        }
      }),

    dequeueConfirmation: () =>
      set((state) => {
        state.pendingConfirmationQueue.shift();
      }),

    removeFromConfirmationQueue: (sessionId) =>
      set((state) => {
        state.pendingConfirmationQueue = state.pendingConfirmationQueue.filter(
          (id) => id !== sessionId
        );
      }),

    setAutoAcceptPlans: (value) =>
      set((state) => {
        state.autoAcceptPlans = value;
      }),

    addAutoAcceptSession: (sessionId) =>
      set((state) => {
        state.autoAcceptSessions.add(sessionId);
      }),

    removeAutoAcceptSession: (sessionId) =>
      set((state) => {
        state.autoAcceptSessions.delete(sessionId);
      }),

  }))
);

// Expose uiStore to window in web mode for Playwright testing
if (typeof window !== "undefined" && !window.__TAURI_INTERNALS__) {
  window.__uiStore = useUiStore;
}

// One-time feature flag initialization on module load.
// Zustand stores cannot use React hooks, so flags are fetched via invoke directly.
// Defaults to the app's startup flag baseline until the async fetch resolves.
// Errors are silently ignored — those defaults remain active.
void invoke<unknown>("get_ui_feature_flags")
  .then((raw) => {
    const result = featureFlagsSchema.safeParse(raw);
    if (result.success) {
      useUiStore.getState().setFeatureFlags(applyFeatureFlagOverrides(result.data));
    }
  })
  .catch(() => {
    // Keep all-enabled defaults on error
  });
