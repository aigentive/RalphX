/**
 * App.tsx project switch integration tests
 *
 * Tests the useEffect logic that watches activeProjectId changes:
 * - switchToProject() clears ephemeral state atomically
 * - Per-project ideation session is restored with validation
 * - Stale/deleted sessions fall back to null
 * - Battle mode is reset on switch
 * - Rapid A→B→A restores the correct view
 */

import { describe, it, expect, beforeEach } from "vitest";
import { useUiStore } from "@/stores/uiStore";
import { useIdeationStore } from "@/stores/ideationStore";

// ============================================================================
// Helpers
// ============================================================================

/**
 * Simulates the App.tsx useEffect project-switch logic without mounting the
 * component. Steps mirror the effect body exactly so tests stay in sync with
 * the real implementation.
 */
function simulateProjectSwitch(prevId: string | null, newId: string) {
  // Step 1: atomic view-state save / clean / restore
  useUiStore.getState().switchToProject(prevId, newId);

  // Step 2: restore ideation session (separate store, same synchronous tick)
  const sessionByProject = useUiStore.getState().sessionByProject;
  const restoredSessionId = sessionByProject[newId] ?? null;

  if (restoredSessionId) {
    const sessions = useIdeationStore.getState().sessions;
    if (sessions[restoredSessionId]) {
      useIdeationStore.getState().setActiveSession(restoredSessionId);
    } else {
      // Session deleted / not yet loaded — don't restore stale ID
      useIdeationStore.getState().setActiveSession(null);
    }
  } else {
    useIdeationStore.getState().setActiveSession(null);
  }
}

function makeSession(id: string, projectId: string) {
  return {
    id,
    projectId,
    title: `Session ${id}`,
    status: "active" as const,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
  };
}

function resetStores() {
  useUiStore.setState({
    currentView: "agents",
    selectedTaskId: null,
    graphSelection: null,
    taskHistoryState: null,
    boardSearchQuery: null,
    battleModeActive: false,
    battleModePanelRestoreState: null,
    activityFilter: { taskId: null, sessionId: null },
    graphRightPanelUserOpen: false,
    graphRightPanelCompactOpen: false,
    viewByProject: {},
    sessionByProject: {},
    selectedTaskByProject: {},
  });

  useIdeationStore.setState({
    sessions: {},
    activeSessionId: null,
    isLoading: false,
    error: null,
  });
}

// ============================================================================
// Tests
// ============================================================================

describe("Project switch effect — App.tsx integration", () => {
  beforeEach(() => {
    resetStores();
  });

  it("clears selectedTaskId when switching projects", () => {
    useUiStore.setState({ selectedTaskId: "task-123", currentView: "task_detail" });

    simulateProjectSwitch("project-a", "project-b");

    expect(useUiStore.getState().selectedTaskId).toBeNull();
  });

  it("restores selected task detail in Agents when switching back to a saved graph route", () => {
    useUiStore.setState({
      currentView: "kanban",
      viewByProject: { "project-b": "graph" },
      selectedTaskByProject: { "project-b": "task-b" },
    });

    simulateProjectSwitch("project-a", "project-b");

    const state = useUiStore.getState();
    expect(state.currentView).toBe("agents");
    expect(state.selectedTaskId).toBe("task-b");
    expect(state.graphSelection).toEqual({ kind: "task", id: "task-b" });
  });

  it("restores ideation session when the saved session exists in the store", () => {
    const sessionId = "session-b-1";
    useIdeationStore.getState().addSession(makeSession(sessionId, "project-b"));
    useUiStore.setState({ sessionByProject: { "project-b": sessionId } });

    simulateProjectSwitch("project-a", "project-b");

    expect(useIdeationStore.getState().activeSessionId).toBe(sessionId);
  });

  it("falls back to null when the saved session has been deleted", () => {
    // sessionByProject points to a session that no longer exists in ideationStore
    useUiStore.setState({ sessionByProject: { "project-b": "deleted-session-id" } });

    simulateProjectSwitch("project-a", "project-b");

    expect(useIdeationStore.getState().activeSessionId).toBeNull();
  });

  it("resets battleModeActive and battleModePanelRestoreState when switching projects", () => {
    useUiStore.setState({
      battleModeActive: true,
      battleModePanelRestoreState: { userOpen: true, compactOpen: false },
    });

    simulateProjectSwitch("project-a", "project-b");

    expect(useUiStore.getState().battleModeActive).toBe(false);
    expect(useUiStore.getState().battleModePanelRestoreState).toBeNull();
  });

  it("restores the correct view on rapid A→B→A switching", () => {
    // Project A is on "activity" view
    useUiStore.setState({ currentView: "activity" });

    // Switch A→B: saves A's "activity", restores default "agents" for B
    simulateProjectSwitch("project-a", "project-b");
    expect(useUiStore.getState().currentView).toBe("agents");

    // User navigates to "insights" within project B
    useUiStore.setState({ currentView: "insights" });

    // Switch B→A: saves B's "insights", restores A's "activity"
    simulateProjectSwitch("project-b", "project-a");
    expect(useUiStore.getState().currentView).toBe("activity");
  });

  it("falls back stale 'settings' localStorage value to agents on project switch", () => {
    // Simulate a stale "settings" value in viewByProject (from before the refactor)
    useUiStore.setState({
      viewByProject: { "project-b": "settings" as never },
    });

    simulateProjectSwitch("project-a", "project-b");
    // "settings" is no longer a valid view — should fall back to "agents"
    expect(useUiStore.getState().currentView).toBe("agents");
  });
});
