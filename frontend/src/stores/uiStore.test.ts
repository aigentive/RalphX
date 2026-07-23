import { describe, it, expect, beforeEach, vi } from "vitest";
import { useUiStore } from "./uiStore";
import type { AskUserQuestionPayload } from "@/types/ask-user-question";
import type { FeatureFlags } from "@/types/feature-flags";

const ALL_ENABLED: FeatureFlags = {
  activityPage: true,
  extensibilityPage: true,
  automationsPage: true,
  atlassianOauth: false,
};

// ============================================================================
// Mocks for per-project route persistence (cross-store reads)
// ============================================================================

const { mockProjectGetState } = vi.hoisted(() => ({
  mockProjectGetState: vi.fn().mockReturnValue({ activeProjectId: null }),
}));

vi.mock("@/stores/projectStore", () => ({
  useProjectStore: { getState: mockProjectGetState },
}));

describe("uiStore", () => {
  beforeEach(() => {
    // Reset store to initial state before each test
    useUiStore.setState({
      sidebarOpen: true,
      notificationsPanelOpen: false,
      currentView: "agents",
      activeModal: null,
      modalContext: undefined,
      notifications: [],
      loading: {},
      confirmation: null,
      activeQuestions: {},
      answeredQuestions: {},
      graphSelection: null,
      graphRightPanelUserOpen: true,
      graphRightPanelCompactOpen: false,
      executionStatus: {
        isPaused: false,
        haltMode: "running",
        runningCount: 0,
        maxConcurrent: 10,
        globalMaxConcurrent: 20,
        queuedCount: 0,
        queuedMessageCount: 0,
        canStartTask: true,
      },
      executionBarOpenPopover: null,
      executionBarRunningTab: "execution",
      viewByProject: {},
      taskHistoryState: null,
      boardSearchQuery: null,
      kanbanCardDisplayMode: "default",
      activityFilter: { taskId: null, sessionId: null },
      featureFlags: ALL_ENABLED,
      preserveCurrentViewOnProjectSwitch: false,
    });
    // Clear localStorage to prevent cross-test contamination
    localStorage.clear();
    mockProjectGetState.mockReturnValue({ activeProjectId: null });
  });

  describe("sidebar", () => {
    it("toggles sidebar visibility", () => {
      expect(useUiStore.getState().sidebarOpen).toBe(true);

      useUiStore.getState().toggleSidebar();
      expect(useUiStore.getState().sidebarOpen).toBe(false);

      useUiStore.getState().toggleSidebar();
      expect(useUiStore.getState().sidebarOpen).toBe(true);
    });

    it("sets sidebar visibility directly", () => {
      useUiStore.getState().setSidebarOpen(false);
      expect(useUiStore.getState().sidebarOpen).toBe(false);

      useUiStore.getState().setSidebarOpen(true);
      expect(useUiStore.getState().sidebarOpen).toBe(true);
    });
  });

  describe("currentView", () => {
    it("initializes with agents view", () => {
      const state = useUiStore.getState();
      expect(state.currentView).toBe("agents");
    });

    it("sets current view to activity", () => {
      useUiStore.getState().setCurrentView("activity");
      expect(useUiStore.getState().currentView).toBe("activity");
    });

    it("switches between live root views", () => {
      useUiStore.getState().setCurrentView("insights");
      expect(useUiStore.getState().currentView).toBe("insights");

      useUiStore.getState().setCurrentView("agents");
      expect(useUiStore.getState().currentView).toBe("agents");
    });
  });

  describe("modal", () => {
    it("opens a modal with type", () => {
      useUiStore.getState().openModal("task-create");

      const state = useUiStore.getState();
      expect(state.activeModal).toBe("task-create");
    });

    it("opens a modal with context", () => {
      useUiStore.getState().openModal("task-create", { taskId: "task-1" });

      const state = useUiStore.getState();
      expect(state.activeModal).toBe("task-create");
      expect(state.modalContext).toEqual({ taskId: "task-1" });
    });

    it("closes the modal", () => {
      useUiStore.setState({
        activeModal: "task-create",
        modalContext: { taskId: "task-1" },
      });

      useUiStore.getState().closeModal();

      const state = useUiStore.getState();
      expect(state.activeModal).toBeNull();
      expect(state.modalContext).toBeUndefined();
    });

    it("replaces modal when opening new one", () => {
      useUiStore.getState().openModal("task-create");
      useUiStore.getState().openModal("settings");

      const state = useUiStore.getState();
      expect(state.activeModal).toBe("settings");
    });
  });

  describe("notifications", () => {
    it("adds a notification", () => {
      useUiStore.getState().addNotification({
        id: "notif-1",
        type: "success",
        message: "Task completed",
      });

      const state = useUiStore.getState();
      expect(state.notifications).toHaveLength(1);
      expect(state.notifications[0]?.message).toBe("Task completed");
    });

    it("adds multiple notifications", () => {
      useUiStore.getState().addNotification({
        id: "notif-1",
        type: "success",
        message: "First",
      });
      useUiStore.getState().addNotification({
        id: "notif-2",
        type: "error",
        message: "Second",
      });

      const state = useUiStore.getState();
      expect(state.notifications).toHaveLength(2);
    });

    it("removes a notification by id", () => {
      useUiStore.setState({
        notifications: [
          { id: "notif-1", type: "success", message: "First" },
          { id: "notif-2", type: "error", message: "Second" },
        ],
      });

      useUiStore.getState().removeNotification("notif-1");

      const state = useUiStore.getState();
      expect(state.notifications).toHaveLength(1);
      expect(state.notifications[0]?.id).toBe("notif-2");
    });

    it("clears all notifications", () => {
      useUiStore.setState({
        notifications: [
          { id: "notif-1", type: "success", message: "First" },
          { id: "notif-2", type: "error", message: "Second" },
        ],
      });

      useUiStore.getState().clearNotifications();

      const state = useUiStore.getState();
      expect(state.notifications).toHaveLength(0);
    });

    it("does nothing when removing nonexistent notification", () => {
      useUiStore.setState({
        notifications: [{ id: "notif-1", type: "success", message: "First" }],
      });

      useUiStore.getState().removeNotification("nonexistent");

      const state = useUiStore.getState();
      expect(state.notifications).toHaveLength(1);
    });
  });

  describe("loading state", () => {
    it("sets loading state", () => {
      useUiStore.getState().setLoading("tasks", true);

      const state = useUiStore.getState();
      expect(state.loading.tasks).toBe(true);
    });

    it("clears loading state", () => {
      useUiStore.setState({ loading: { tasks: true } });

      useUiStore.getState().setLoading("tasks", false);

      const state = useUiStore.getState();
      expect(state.loading.tasks).toBe(false);
    });

    it("tracks multiple loading states", () => {
      useUiStore.getState().setLoading("tasks", true);
      useUiStore.getState().setLoading("projects", true);

      const state = useUiStore.getState();
      expect(state.loading.tasks).toBe(true);
      expect(state.loading.projects).toBe(true);
    });
  });

  describe("confirmation dialog", () => {
    it("shows confirmation dialog", () => {
      useUiStore.getState().showConfirmation({
        title: "Delete Task",
        message: "Are you sure?",
        onConfirm: () => {},
      });

      const state = useUiStore.getState();
      expect(state.confirmation).toBeDefined();
      expect(state.confirmation?.title).toBe("Delete Task");
    });

    it("hides confirmation dialog", () => {
      useUiStore.setState({
        confirmation: {
          title: "Test",
          message: "Test",
          onConfirm: () => {},
        },
      });

      useUiStore.getState().hideConfirmation();

      const state = useUiStore.getState();
      expect(state.confirmation).toBeNull();
    });
  });

  describe("active question (per-session)", () => {
    const sessionId = "session-abc";
    const mockQuestion: AskUserQuestionPayload = {
      requestId: "req-123",
      taskId: "task-123",
      sessionId,
      question: "Which authentication method should we use?",
      header: "Auth method",
      options: [
        { label: "JWT tokens", description: "Use JSON Web Tokens" },
        { label: "Session cookies", description: "Use server-side sessions" },
      ],
      multiSelect: false,
    };

    it("sets active question for session", () => {
      useUiStore.getState().setActiveQuestion(sessionId, mockQuestion);

      const state = useUiStore.getState();
      expect(state.activeQuestions[sessionId]).toEqual(mockQuestion);
    });

    it("clears active question for session", () => {
      useUiStore.getState().setActiveQuestion(sessionId, mockQuestion);
      useUiStore.getState().clearActiveQuestion(sessionId);

      const state = useUiStore.getState();
      expect(state.activeQuestions[sessionId]).toBeUndefined();
    });

    it("replaces existing question for same session", () => {
      useUiStore.getState().setActiveQuestion(sessionId, mockQuestion);

      const newQuestion: AskUserQuestionPayload = {
        requestId: "req-456",
        taskId: "task-456",
        sessionId,
        question: "Which database?",
        header: "Database",
        options: [
          { label: "PostgreSQL", description: "Relational database" },
          { label: "MongoDB", description: "Document database" },
        ],
        multiSelect: false,
      };

      useUiStore.getState().setActiveQuestion(sessionId, newQuestion);

      const state = useUiStore.getState();
      expect(state.activeQuestions[sessionId]?.taskId).toBe("task-456");
      expect(state.activeQuestions[sessionId]?.question).toBe("Which database?");
    });

    it("initializes with empty activeQuestions", () => {
      const state = useUiStore.getState();
      expect(Object.keys(state.activeQuestions)).toHaveLength(0);
    });

    it("preserves multiSelect in question", () => {
      const multiSelectQuestion: AskUserQuestionPayload = {
        ...mockQuestion,
        multiSelect: true,
      };

      useUiStore.getState().setActiveQuestion(sessionId, multiSelectQuestion);

      const state = useUiStore.getState();
      expect(state.activeQuestions[sessionId]?.multiSelect).toBe(true);
    });

    it("dismissQuestion clears both question and answered for session", () => {
      useUiStore.getState().setActiveQuestion(sessionId, mockQuestion);
      useUiStore.getState().setAnsweredQuestion(sessionId, "JWT tokens");

      useUiStore.getState().dismissQuestion(sessionId);

      const state = useUiStore.getState();
      expect(state.activeQuestions[sessionId]).toBeUndefined();
      expect(state.answeredQuestions[sessionId]).toBeUndefined();
    });

    it("setAnsweredQuestion stores per-session summary", () => {
      useUiStore.getState().setAnsweredQuestion(sessionId, "JWT tokens");

      const state = useUiStore.getState();
      expect(state.answeredQuestions[sessionId]).toBe("JWT tokens");
    });

    it("clearAnsweredQuestion removes session summary", () => {
      useUiStore.getState().setAnsweredQuestion(sessionId, "JWT tokens");
      useUiStore.getState().clearAnsweredQuestion(sessionId);

      const state = useUiStore.getState();
      expect(state.answeredQuestions[sessionId]).toBeUndefined();
    });
  });

  describe("execution state", () => {
    it("initializes with default execution state", () => {
      const state = useUiStore.getState();
      expect(state.executionStatus).toEqual({
        isPaused: false,
        haltMode: "running",
        runningCount: 0,
        maxConcurrent: 10,
        globalMaxConcurrent: 20,
        queuedCount: 0,
        queuedMessageCount: 0,
        canStartTask: true,
      });
    });

    it("updates execution status", () => {
      useUiStore.getState().setExecutionStatus({
        isPaused: true,
        haltMode: "paused",
        runningCount: 1,
        maxConcurrent: 10,
        globalMaxConcurrent: 20,
        queuedCount: 3,
        queuedMessageCount: 2,
        canStartTask: false,
      });

      const state = useUiStore.getState();
      expect(state.executionStatus.isPaused).toBe(true);
      expect(state.executionStatus.runningCount).toBe(1);
      expect(state.executionStatus.queuedCount).toBe(3);
      expect(state.executionStatus.queuedMessageCount).toBe(2);
      expect(state.executionStatus.canStartTask).toBe(false);
    });

    it("sets paused state directly", () => {
      useUiStore.getState().setExecutionPaused(true);

      const state = useUiStore.getState();
      expect(state.executionStatus.isPaused).toBe(true);

      useUiStore.getState().setExecutionPaused(false);
      expect(useUiStore.getState().executionStatus.isPaused).toBe(false);
    });

    it("updates running count", () => {
      useUiStore.getState().setExecutionRunningCount(2);

      const state = useUiStore.getState();
      expect(state.executionStatus.runningCount).toBe(2);
    });

    it("updates queued count", () => {
      useUiStore.getState().setExecutionQueuedCount(5);

      const state = useUiStore.getState();
      expect(state.executionStatus.queuedCount).toBe(5);
    });

    it("partial update preserves other fields", () => {
      useUiStore.getState().setExecutionStatus({
        isPaused: true,
        haltMode: "paused",
        runningCount: 1,
        maxConcurrent: 4,
        globalMaxConcurrent: 20,
        queuedCount: 10,
        queuedMessageCount: 4,
        canStartTask: false,
      });

      useUiStore.getState().setExecutionPaused(false);

      const state = useUiStore.getState();
      expect(state.executionStatus.isPaused).toBe(false);
      expect(state.executionStatus.runningCount).toBe(1);
      expect(state.executionStatus.queuedCount).toBe(10);
      expect(state.executionStatus.queuedMessageCount).toBe(4);
    });
  });

  describe("execution bar popover state", () => {
    it("stores the open execution bar popover and running tab outside the footer component", () => {
      useUiStore.getState().setExecutionBarOpenPopover("running");
      useUiStore.getState().setExecutionBarRunningTab("workspaces");

      expect(useUiStore.getState().executionBarOpenPopover).toBe("running");
      expect(useUiStore.getState().executionBarRunningTab).toBe("workspaces");

      useUiStore.getState().setExecutionBarOpenPopover("terminals");
      expect(useUiStore.getState().executionBarOpenPopover).toBe("terminals");
    });

    it("preserves execution bar popover state when switching projects", () => {
      useUiStore.setState({
        currentView: "agents",
        executionBarOpenPopover: "queued",
        executionBarRunningTab: "ideation",
        viewByProject: {},
      });

      useUiStore.getState().switchToProject("proj-a", "proj-b");

      expect(useUiStore.getState().executionBarOpenPopover).toBe("queued");
      expect(useUiStore.getState().executionBarRunningTab).toBe("ideation");
    });
  });

  describe("graphSelection", () => {
    it("sets and clears non-task selection", () => {
      useUiStore.getState().setGraphSelection({ kind: "planGroup", id: "plan-1" });
      expect(useUiStore.getState().graphSelection).toEqual({ kind: "planGroup", id: "plan-1" });

      useUiStore.getState().clearGraphSelection();
      expect(useUiStore.getState().graphSelection).toBeNull();
    });
  });

  describe("taskCreationContext", () => {
    it("stores optional flow context when opening task creation", () => {
      useUiStore.getState().openTaskCreation("project-1", "New task", {
        ideationSessionId: "session-1",
        executionPlanId: "exec-plan-1",
      });

      expect(useUiStore.getState().taskCreationContext).toEqual({
        projectId: "project-1",
        defaultTitle: "New task",
        ideationSessionId: "session-1",
        executionPlanId: "exec-plan-1",
      });
    });

    it("closes task creation context", () => {
      useUiStore.getState().openTaskCreation("project-1", "New task");
      useUiStore.getState().closeTaskCreation();

      expect(useUiStore.getState().taskCreationContext).toBeNull();
    });
  });

  // ============================================================================
  // Per-Project Route Persistence
  // ============================================================================

  describe("switchToProject", () => {
    const PROJECT_A = "proj-a";
    const PROJECT_B = "proj-b";

    it("saves the current live view to viewByProject for old project", () => {
      useUiStore.setState({ currentView: "activity", viewByProject: {} });

      useUiStore.getState().switchToProject(PROJECT_A, PROJECT_B);

      expect(useUiStore.getState().viewByProject[PROJECT_A]).toBe("activity");
    });

    it("restores a saved live view for the new project from the map", () => {
      useUiStore.setState({
        currentView: "agents",
        viewByProject: { [PROJECT_B]: "insights" },
      });

      useUiStore.getState().switchToProject(PROJECT_A, PROJECT_B);

      expect(useUiStore.getState().currentView).toBe("insights");
    });

    it("preserves the current section for one top-bar project switch", () => {
      useUiStore.setState({
        currentView: "github",
        viewByProject: { [PROJECT_B]: "automations" },
      });

      useUiStore.getState().preserveCurrentViewOnNextProjectSwitch();
      useUiStore.getState().switchToProject(PROJECT_A, PROJECT_B);

      expect(useUiStore.getState().currentView).toBe("github");
      expect(useUiStore.getState().viewByProject[PROJECT_B]).toBe("github");
      expect(useUiStore.getState().preserveCurrentViewOnProjectSwitch).toBe(false);

      useUiStore.setState({
        currentView: "granola",
        viewByProject: {
          ...useUiStore.getState().viewByProject,
          [PROJECT_A]: "automations",
        },
      });

      useUiStore.getState().switchToProject(PROJECT_B, PROJECT_A);
      expect(useUiStore.getState().currentView).toBe("automations");
    });

    it("restores saved insights view for new project from map", () => {
      useUiStore.setState({
        currentView: "agents",
        viewByProject: { [PROJECT_B]: "insights" },
      });

      useUiStore.getState().switchToProject(PROJECT_A, PROJECT_B);

      expect(useUiStore.getState().currentView).toBe("insights");
    });

    it("defaults to agents when new project has no saved view", () => {
      useUiStore.setState({ currentView: "activity", viewByProject: {} });

      useUiStore.getState().switchToProject(PROJECT_A, PROJECT_B);

      expect(useUiStore.getState().currentView).toBe("agents");
    });

    it("clears ephemeral state when the new project has no saved task detail", () => {
      useUiStore.setState({
        graphSelection: { kind: "task", id: "task-1" },
        taskHistoryState: { status: "backlog", timestamp: "2026-01-01T00:00:00Z" },
        boardSearchQuery: "some query",
        activityFilter: { taskId: "task-1", sessionId: "session-1" },
        graphRightPanelUserOpen: true,
        graphRightPanelCompactOpen: true,
      });

      useUiStore.getState().switchToProject(PROJECT_A, PROJECT_B);

      const state = useUiStore.getState();
      expect(state.graphSelection).toBeNull();
      expect(state.taskHistoryState).toBeNull();
      expect(state.boardSearchQuery).toBeNull();
      expect(state.activityFilter).toEqual({ taskId: null, sessionId: null });
      expect(state.graphRightPanelUserOpen).toBe(false);
      expect(state.graphRightPanelCompactOpen).toBe(false);
    });

    it("null oldProjectId skips save phase (first load)", () => {
      useUiStore.setState({ currentView: "activity", viewByProject: {} });

      useUiStore.getState().switchToProject(null, PROJECT_B);

      const state = useUiStore.getState();
      // No entry should have been saved for "null" or anything unexpected
      expect(Object.keys(state.viewByProject)).not.toContain("null");
      expect(Object.keys(state.viewByProject)).toHaveLength(0);
    });

    it("persists viewByProject to localStorage", () => {
      useUiStore.setState({ currentView: "activity" });

      useUiStore.getState().switchToProject(PROJECT_A, PROJECT_B);

      const stored = localStorage.getItem("ralphx-views-by-project");
      expect(stored).not.toBeNull();
      const parsed = JSON.parse(stored!) as Record<string, string>;
      expect(parsed[PROJECT_A]).toBe("activity");
    });
  });

  describe("setCurrentView write-through", () => {
    it("updates viewByProject for active project on view change", () => {
      mockProjectGetState.mockReturnValue({ activeProjectId: "proj-a" });
      useUiStore.setState({ viewByProject: {} });

      useUiStore.getState().setCurrentView("activity");

      const state = useUiStore.getState();
      expect(state.currentView).toBe("activity");
      expect(state.viewByProject["proj-a"]).toBe("activity");
    });

    it("persists view to localStorage when active project is set", () => {
      mockProjectGetState.mockReturnValue({ activeProjectId: "proj-a" });
      useUiStore.setState({ viewByProject: {} });

      useUiStore.getState().setCurrentView("insights");

      const stored = localStorage.getItem("ralphx-views-by-project");
      expect(stored).not.toBeNull();
      const parsed = JSON.parse(stored!) as Record<string, string>;
      expect(parsed["proj-a"]).toBe("insights");
    });

    it("does not create viewByProject entry when activeProjectId is null", () => {
      mockProjectGetState.mockReturnValue({ activeProjectId: null });
      useUiStore.setState({ viewByProject: {} });

      useUiStore.getState().setCurrentView("activity");

      const state = useUiStore.getState();
      expect(state.currentView).toBe("activity");
      // No null key should appear in the map
      expect(Object.keys(state.viewByProject)).not.toContain("null");
      expect(Object.keys(state.viewByProject)).toHaveLength(0);
    });

    it("does not write to localStorage when activeProjectId is null", () => {
      mockProjectGetState.mockReturnValue({ activeProjectId: null });

      useUiStore.getState().setCurrentView("activity");

      // No view entry should be persisted for null project
      expect(localStorage.getItem("ralphx-views-by-project")).toBeNull();
    });
  });

  describe("cleanupProjectRoute", () => {
    it("removes view entry for a deleted project", () => {
      useUiStore.setState({
        viewByProject: { "proj-a": "activity", "proj-b": "insights" },
      });

      useUiStore.getState().cleanupProjectRoute("proj-a");

      const state = useUiStore.getState();
      expect(state.viewByProject["proj-a"]).toBeUndefined();
      expect(state.viewByProject["proj-b"]).toBe("insights");
    });

    it("persists cleaned viewByProject to localStorage", () => {
      useUiStore.setState({
        viewByProject: { "proj-a": "activity", "proj-b": "insights" },
      });

      useUiStore.getState().cleanupProjectRoute("proj-a");

      const stored = localStorage.getItem("ralphx-views-by-project");
      expect(stored).not.toBeNull();
      const parsed = JSON.parse(stored!) as Record<string, string>;
      expect(Object.keys(parsed)).not.toContain("proj-a");
      expect(parsed["proj-b"]).toBe("insights");
    });

    it("is a no-op for a project that has no saved route", () => {
      useUiStore.setState({
        viewByProject: { "proj-b": "insights" },
      });

      expect(() => useUiStore.getState().cleanupProjectRoute("proj-unknown")).not.toThrow();

      expect(useUiStore.getState().viewByProject["proj-b"]).toBe("insights");
    });
  });

  describe("Kanban card display mode", () => {
    it("persists the app-wide card display preference to localStorage", () => {
      useUiStore.getState().setKanbanCardDisplayMode("mini");

      expect(useUiStore.getState().kanbanCardDisplayMode).toBe("mini");
      expect(localStorage.getItem("ralphx-kanban-card-display-mode")).toBe("mini");
    });
  });

  describe("localStorage helpers", () => {
    it("returns empty map when localStorage key is missing", () => {
      // Ensure key is absent
      localStorage.removeItem("ralphx-views-by-project");
      localStorage.removeItem("ralphx-sessions-by-project");
      localStorage.removeItem("ralphx-selected-task-by-project");

      // Simulate what happens when store re-initializes with empty localStorage:
      // switchToProject with no pre-existing data should work fine
      useUiStore.getState().switchToProject(null, "proj-a");

      expect(useUiStore.getState().currentView).toBe("agents");
      expect(useUiStore.getState().viewByProject).toBeDefined();
    });

    it("rewrites corrupt persisted routes to an empty normalized map", async () => {
      localStorage.setItem("ralphx-views-by-project", "not-valid-json{{{");

      vi.resetModules();
      const { useUiStore: reloadedStore } = await import("./uiStore");

      expect(reloadedStore.getState().viewByProject).toEqual({});
      expect(localStorage.getItem("ralphx-views-by-project")).toBe("{}");
    });

    it("silently catches localStorage write failure in switchToProject", () => {
      const setItemSpy = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
        throw new DOMException("QuotaExceededError");
      });

      expect(() => {
        useUiStore.getState().switchToProject("proj-a", "proj-b");
      }).not.toThrow();

      setItemSpy.mockRestore();
    });

    it("silently catches localStorage write failure in setCurrentView", () => {
      mockProjectGetState.mockReturnValue({ activeProjectId: "proj-a" });
      const setItemSpy = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
        throw new DOMException("QuotaExceededError");
      });

      expect(() => {
        useUiStore.getState().setCurrentView("activity");
      }).not.toThrow();

      setItemSpy.mockRestore();
    });

    it("silently catches localStorage write failure in cleanupProjectRoute", () => {
      useUiStore.setState({
        viewByProject: { "proj-a": "activity" },
      });
      const setItemSpy = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
        throw new DOMException("QuotaExceededError");
      });

      expect(() => {
        useUiStore.getState().cleanupProjectRoute("proj-a");
      }).not.toThrow();

      setItemSpy.mockRestore();
    });

    it("normalizes legacy AppView routes and clears retired route storage on initialization", async () => {
      localStorage.setItem(
        "ralphx-views-by-project",
        JSON.stringify({ "proj-a": "graph", "proj-b": "activity" }),
      );
      localStorage.setItem("ralphx-sessions-by-project", JSON.stringify({ "proj-a": "session-1" }));
      localStorage.setItem("ralphx-selected-task-by-project", JSON.stringify({ "proj-a": "task-1" }));

      vi.resetModules();
      await import("./uiStore");

      expect(JSON.parse(localStorage.getItem("ralphx-views-by-project") ?? "{}"))
        .toEqual({ "proj-a": "agents", "proj-b": "activity" });
      expect(localStorage.getItem("ralphx-sessions-by-project")).toBeNull();
      expect(localStorage.getItem("ralphx-selected-task-by-project")).toBeNull();
    });
  });

  describe("rapid project switching", () => {
    it("A->B->A restores A's normalized original view correctly", () => {
      const PROJECT_A = "proj-a";
      const PROJECT_B = "proj-b";

      // Start on A with an activity view
      useUiStore.setState({ currentView: "activity", viewByProject: {} });

      // Switch to B (saves A's activity view, B defaults to agents)
      useUiStore.getState().switchToProject(PROJECT_A, PROJECT_B);
      expect(useUiStore.getState().currentView).toBe("agents");

      // Switch to A (saves B's agents view, restores A's activity view)
      useUiStore.getState().switchToProject(PROJECT_B, PROJECT_A);
      expect(useUiStore.getState().currentView).toBe("activity");

      expect(useUiStore.getState().viewByProject[PROJECT_A]).toBe("activity");
    });

    it("A->B->C preserves each project's normalized view independently", () => {
      const PROJECT_A = "proj-a";
      const PROJECT_B = "proj-b";
      const PROJECT_C = "proj-c";

      // Set up: A has automations, B has insights, and C has activity.
      useUiStore.setState({
        currentView: "automations",
        viewByProject: { [PROJECT_B]: "insights", [PROJECT_C]: "activity" },
      });

      // A→B
      useUiStore.getState().switchToProject(PROJECT_A, PROJECT_B);
      expect(useUiStore.getState().currentView).toBe("insights");

      // B→C
      useUiStore.getState().switchToProject(PROJECT_B, PROJECT_C);
      expect(useUiStore.getState().currentView).toBe("activity");

      expect(useUiStore.getState().viewByProject[PROJECT_B]).toBe("insights");
      expect(useUiStore.getState().viewByProject[PROJECT_A]).toBe("automations");
    });
  });

  // ============================================================================
  // Feature Flag Guards
  // ============================================================================

  describe("feature flag guards", () => {
    describe("setCurrentView", () => {
      it("redirects to agents when activity page is disabled", () => {
        useUiStore.setState({
          featureFlags: { ...ALL_ENABLED, activityPage: false },
        });

        useUiStore.getState().setCurrentView("activity");

        expect(useUiStore.getState().currentView).toBe("agents");
      });

      it("redirects to agents when extensibility page is disabled", () => {
        useUiStore.setState({
          featureFlags: { ...ALL_ENABLED, extensibilityPage: false },
        });

        useUiStore.getState().setCurrentView("extensibility");

        expect(useUiStore.getState().currentView).toBe("agents");
      });

      it("redirects to agents when automations page is disabled", () => {
        useUiStore.setState({
          featureFlags: { ...ALL_ENABLED, automationsPage: false },
        });

        useUiStore.getState().setCurrentView("automations");

        expect(useUiStore.getState().currentView).toBe("agents");
      });

      it("allows activity when activity page is enabled", () => {
        useUiStore.setState({ featureFlags: ALL_ENABLED });

        useUiStore.getState().setCurrentView("activity");

        expect(useUiStore.getState().currentView).toBe("activity");
      });

      it("allows extensibility when extensibility page is enabled", () => {
        useUiStore.setState({ featureFlags: ALL_ENABLED });

        useUiStore.getState().setCurrentView("extensibility");

        expect(useUiStore.getState().currentView).toBe("extensibility");
      });

      it("allows ticketing navigation so the dashboard access can open its screen", () => {
        useUiStore.setState({
          featureFlags: {
            activityPage: true,
            extensibilityPage: true,
            automationsPage: false,
            atlassianOauth: false,
            ticketingDashboard: false,
          },
        });

        useUiStore.getState().setCurrentView("ticketing");

        expect(useUiStore.getState().currentView).toBe("ticketing");
      });

      it("allows ticketing navigation when the dashboard flag is enabled", () => {
        useUiStore.setState({
          featureFlags: {
            activityPage: true,
            extensibilityPage: true,
            automationsPage: false,
            atlassianOauth: false,
            ticketingDashboard: true,
          },
        });

        useUiStore.getState().setCurrentView("ticketing");

        expect(useUiStore.getState().currentView).toBe("ticketing");
      });

      it("allows automations when automations page is enabled", () => {
        useUiStore.setState({ featureFlags: ALL_ENABLED });

        useUiStore.getState().setCurrentView("automations");

        expect(useUiStore.getState().currentView).toBe("automations");
      });

      it("does not persist disabled view to viewByProject", () => {
        mockProjectGetState.mockReturnValue({ activeProjectId: "proj-a" });
        useUiStore.setState({
          featureFlags: { ...ALL_ENABLED, activityPage: false },
          viewByProject: {},
        });

        useUiStore.getState().setCurrentView("activity");

        // viewByProject should store agents (the redirected view), not activity
        expect(useUiStore.getState().viewByProject["proj-a"]).toBe("agents");
      });
    });

    describe("setFeatureFlags", () => {
      it("moves the active project back to agents when the current view becomes disabled", () => {
        mockProjectGetState.mockReturnValue({ activeProjectId: "proj-a" });
        useUiStore.setState({
          currentView: "activity",
          viewByProject: { "proj-a": "activity" },
        });

        useUiStore.getState().setFeatureFlags({ ...ALL_ENABLED, activityPage: false });

        expect(useUiStore.getState().currentView).toBe("agents");
        expect(useUiStore.getState().viewByProject["proj-a"]).toBe("agents");
        expect(JSON.parse(localStorage.getItem("ralphx-views-by-project") ?? "{}")).toEqual({
          "proj-a": "agents",
        });
      });
    });

    describe("switchToProject", () => {
      const PROJECT_A = "proj-a";
      const PROJECT_B = "proj-b";

      it("redirects to agents when restoring a disabled activity view", () => {
        useUiStore.setState({
          featureFlags: { ...ALL_ENABLED, activityPage: false },
          viewByProject: { [PROJECT_B]: "activity" },
        });

        useUiStore.getState().switchToProject(PROJECT_A, PROJECT_B);

        expect(useUiStore.getState().currentView).toBe("agents");
      });

      it("redirects to agents when restoring a disabled extensibility view", () => {
        useUiStore.setState({
          featureFlags: { ...ALL_ENABLED, extensibilityPage: false },
          viewByProject: { [PROJECT_B]: "extensibility" },
        });

        useUiStore.getState().switchToProject(PROJECT_A, PROJECT_B);

        expect(useUiStore.getState().currentView).toBe("agents");
      });

      it("redirects to agents when restoring a disabled automations view", () => {
        useUiStore.setState({
          featureFlags: { ...ALL_ENABLED, automationsPage: false },
          viewByProject: { [PROJECT_B]: "automations" },
        });

        useUiStore.getState().switchToProject(PROJECT_A, PROJECT_B);

        expect(useUiStore.getState().currentView).toBe("agents");
      });

      it("restores activity when activity is enabled", () => {
        useUiStore.setState({
          featureFlags: ALL_ENABLED,
          viewByProject: { [PROJECT_B]: "activity" },
        });

        useUiStore.getState().switchToProject(PROJECT_A, PROJECT_B);

        expect(useUiStore.getState().currentView).toBe("activity");
      });

      it("redirects on initial load (null oldProjectId) with disabled persisted view", () => {
        useUiStore.setState({
          featureFlags: { ...ALL_ENABLED, activityPage: false },
          viewByProject: { [PROJECT_B]: "activity" },
        });

        useUiStore.getState().switchToProject(null, PROJECT_B);

        expect(useUiStore.getState().currentView).toBe("agents");
      });

      it("both flags disabled — persisted activity falls back to agents", () => {
        useUiStore.setState({
          featureFlags: { ...ALL_ENABLED, activityPage: false, extensibilityPage: false },
          viewByProject: { [PROJECT_B]: "activity" },
        });

        useUiStore.getState().switchToProject(PROJECT_A, PROJECT_B);

        expect(useUiStore.getState().currentView).toBe("agents");
      });
    });
  });

});
