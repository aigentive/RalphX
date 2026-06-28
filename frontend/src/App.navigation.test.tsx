/**
 * App navigation integration tests
 *
 * Tests for:
 * - View switching between Kanban and Ideation
 * - Ideation link in navigation
 * - Chat context updates when view changes
 * - Session persistence when navigating
 */

import { describe, it, expect, beforeEach } from "vitest";
import { useUiStore } from "@/stores/uiStore";
import { useChatStore } from "@/stores/chatStore";
import { useIdeationStore } from "@/stores/ideationStore";
import type { FeatureFlags } from "@/types/feature-flags";

const ALL_ENABLED: FeatureFlags = {
  activityPage: true,
  extensibilityPage: true,
  ideationPage: true,
  battleMode: true,
  teamMode: false,
  atlassianOauth: false,
};

// ============================================================================
// Test Setup
// ============================================================================

function resetStores() {
  useUiStore.setState({
    sidebarOpen: true,
    reviewsPanelOpen: false,
    currentView: "agents",
    activeModal: null,
    modalContext: undefined,
    notifications: [],
    loading: {},
    confirmation: null,
    activeQuestions: {},
    answeredQuestions: {},
    selectedTaskId: null,
    graphSelection: null,
    viewByProject: {},
    sessionByProject: {},
    selectedTaskByProject: {},
    featureFlags: ALL_ENABLED,
    executionStatus: {
      isPaused: false,
      runningCount: 0,
      maxConcurrent: 10,
      queuedCount: 0,
      canStartTask: true,
    },
  });

  useChatStore.setState({
    messages: {},
    context: {
      view: "kanban",
      projectId: "demo-project",
    },
    isLoading: false,
  });

  useIdeationStore.setState({
    sessions: {},
    activeSessionId: null,
    isLoading: false,
    error: null,
  });
}

// ============================================================================
// Store Tests
// ============================================================================

describe("Navigation store state", () => {
  beforeEach(() => {
    resetStores();
  });

  describe("currentView state", () => {
    it("initializes with agents view", () => {
      const state = useUiStore.getState();
      expect(state.currentView).toBe("agents");
    });

    it("switches to ideation view", () => {
      useUiStore.getState().setCurrentView("ideation");
      expect(useUiStore.getState().currentView).toBe("ideation");
    });

    it("switches back to kanban view", () => {
      useUiStore.getState().setCurrentView("ideation");
      useUiStore.getState().setCurrentView("kanban");
      expect(useUiStore.getState().currentView).toBe("kanban");
    });

    it("supports all view types", () => {
      const views = ["kanban", "ideation", "activity", "github", "granola", "task_detail"] as const;

      for (const view of views) {
        useUiStore.getState().setCurrentView(view);
        expect(useUiStore.getState().currentView).toBe(view);
      }
    });
  });

  describe("view switching preserves other state", () => {
    it("preserves sidebar state when switching views", () => {
      useUiStore.getState().setSidebarOpen(false);
      useUiStore.getState().setCurrentView("ideation");
      expect(useUiStore.getState().sidebarOpen).toBe(false);
    });

    it("preserves reviews panel state when switching views", () => {
      useUiStore.getState().setReviewsPanelOpen(true);
      useUiStore.getState().setCurrentView("ideation");
      expect(useUiStore.getState().reviewsPanelOpen).toBe(true);
    });

    it("preserves execution status when switching views", () => {
      useUiStore.getState().setExecutionPaused(true);
      useUiStore.getState().setCurrentView("ideation");
      expect(useUiStore.getState().executionStatus.isPaused).toBe(true);
    });
  });

  describe("ideation session state persistence", () => {
    it("preserves active session when navigating away", () => {
      useIdeationStore.getState().addSession({
        id: "session-1",
        projectId: "demo-project",
        title: "Test Session",
        status: "active",
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      });
      useIdeationStore.getState().setActiveSession("session-1");

      // Navigate away and back
      useUiStore.getState().setCurrentView("kanban");
      useUiStore.getState().setCurrentView("ideation");

      // Session should still be active
      expect(useIdeationStore.getState().activeSessionId).toBe("session-1");
    });

    it("preserves sessions when navigating away", () => {
      useIdeationStore.getState().addSession({
        id: "session-1",
        projectId: "demo-project",
        title: "Test Session",
        status: "active",
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      });

      useUiStore.getState().setCurrentView("kanban");

      expect(useIdeationStore.getState().sessions["session-1"]).toBeDefined();
    });
  });
});

// ============================================================================
// View Rendering Logic Tests (without full App render)
// ============================================================================

describe("View rendering logic", () => {
  beforeEach(() => {
    resetStores();
  });

  it("agents view is the initial default", () => {
    expect(useUiStore.getState().currentView).toBe("agents");
  });

  it("can switch to ideation view", () => {
    useUiStore.getState().setCurrentView("ideation");
    expect(useUiStore.getState().currentView).toBe("ideation");
  });

  it("setCurrentView updates currentView correctly", () => {
    const views: Array<"kanban" | "ideation" | "activity" | "github" | "granola" | "task_detail"> = [
      "ideation",
      "kanban",
      "activity",
      "github",
      "granola",
      "task_detail",
    ];

    for (const view of views) {
      useUiStore.getState().setCurrentView(view);
      expect(useUiStore.getState().currentView).toBe(view);
    }
  });
});

// ============================================================================
// Chat Context Logic Tests
// ============================================================================

describe("Chat context logic", () => {
  beforeEach(() => {
    resetStores();
  });

  it("chat context is initialized with kanban view", () => {
    const context = useChatStore.getState().context;
    expect(context.view).toBe("kanban");
    expect(context.projectId).toBe("demo-project");
  });

  it("chat context can be updated to ideation view", () => {
    useChatStore.getState().setContext({
      view: "ideation",
      projectId: "demo-project",
      ideationSessionId: "session-123",
    });

    const context = useChatStore.getState().context;
    expect(context.view).toBe("ideation");
    expect(context.ideationSessionId).toBe("session-123");
  });

});

// ============================================================================
// Integration Contract Tests
// ============================================================================

describe("Navigation integration contracts", () => {
  beforeEach(() => {
    resetStores();
  });

  it("navigation state is independent of ideation state", () => {
    // Set up ideation session
    useIdeationStore.getState().addSession({
      id: "session-1",
      projectId: "demo-project",
      title: "Test Session",
      status: "active",
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    });
    useIdeationStore.getState().setActiveSession("session-1");

    // Switch views - session should persist
    useUiStore.getState().setCurrentView("kanban");
    expect(useIdeationStore.getState().activeSessionId).toBe("session-1");

    useUiStore.getState().setCurrentView("ideation");
    expect(useIdeationStore.getState().activeSessionId).toBe("session-1");
  });

  it("chat store and ui store can be updated independently", () => {
    // Change view
    useUiStore.getState().setCurrentView("ideation");

    // Update chat context
    useChatStore.getState().setContext({
      view: "ideation",
      projectId: "test-project",
      ideationSessionId: "session-1",
    });

    // Both should reflect their respective states
    expect(useUiStore.getState().currentView).toBe("ideation");
    expect(useChatStore.getState().context.view).toBe("ideation");
    expect(useChatStore.getState().context.projectId).toBe("test-project");
  });

  it("view changes do not affect modal state", () => {
    useUiStore.getState().openModal("task-create", { taskId: "task-1" });

    useUiStore.getState().setCurrentView("ideation");

    expect(useUiStore.getState().activeModal).toBe("task-create");
    expect(useUiStore.getState().modalContext).toEqual({ taskId: "task-1" });
  });

  it("view changes do not affect notification state", () => {
    useUiStore.getState().addNotification({
      id: "notif-1",
      type: "success",
      message: "Test notification",
    });

    useUiStore.getState().setCurrentView("ideation");

    expect(useUiStore.getState().notifications).toHaveLength(1);
    expect(useUiStore.getState().notifications[0]?.message).toBe("Test notification");
  });
});
