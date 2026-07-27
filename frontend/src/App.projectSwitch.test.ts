import { beforeEach, describe, expect, it } from "vitest";

import { useUiStore } from "@/stores/uiStore";

function resetStore() {
  useUiStore.setState({
    currentView: "agents",
    graphSelection: null,
    taskHistoryState: null,
    boardSearchQuery: null,
    activityFilter: { taskId: null, sessionId: null },
    graphRightPanelUserOpen: false,
    graphRightPanelCompactOpen: false,
    viewByProject: {},
  });
}

describe("App project switching", () => {
  beforeEach(resetStore);

  it("clears conversation-scoped graph selection and history", () => {
    useUiStore.setState({
      graphSelection: { kind: "task", id: "task-123" },
      taskHistoryState: { status: "backlog", timestamp: "2026-01-01T00:00:00Z" },
    });

    useUiStore.getState().switchToProject("project-a", "project-b");

    expect(useUiStore.getState().graphSelection).toBeNull();
    expect(useUiStore.getState().taskHistoryState).toBeNull();
  });

  it("restores the correct live root view on rapid A→B→A switching", () => {
    useUiStore.setState({ currentView: "activity" });
    useUiStore.getState().switchToProject("project-a", "project-b");
    useUiStore.setState({ currentView: "insights" });
    useUiStore.getState().switchToProject("project-b", "project-a");

    expect(useUiStore.getState().currentView).toBe("activity");
  });
});
