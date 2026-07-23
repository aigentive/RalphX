import { beforeEach, describe, expect, it } from "vitest";

import { useChatStore } from "@/stores/chatStore";
import { useIdeationStore } from "@/stores/ideationStore";
import { useUiStore } from "@/stores/uiStore";
import type { FeatureFlags } from "@/types/feature-flags";

const ALL_ENABLED: FeatureFlags = {
  activityPage: true,
  extensibilityPage: true,
  automationsPage: true,
  atlassianOauth: false,
};

function resetStores() {
  useUiStore.setState({
    currentView: "agents",
    graphSelection: null,
    viewByProject: {},
    featureFlags: ALL_ENABLED,
  });
  useChatStore.setState({
    context: { view: "kanban", projectId: "demo-project" },
  });
  useIdeationStore.setState({
    sessions: {},
    activeSessionId: null,
    isLoading: false,
    error: null,
  });
}

describe("root navigation and chat context boundaries", () => {
  beforeEach(resetStores);

  it("starts at the Agents root and accepts live root views", () => {
    expect(useUiStore.getState().currentView).toBe("agents");

    useUiStore.getState().setCurrentView("activity");
    expect(useUiStore.getState().currentView).toBe("activity");
  });

  it("keeps legacy chat markers independent from root navigation", () => {
    useChatStore.getState().setContext({
      view: "ideation",
      projectId: "demo-project",
      ideationSessionId: "session-123",
    });

    expect(useChatStore.getState().context.view).toBe("ideation");
    expect(useUiStore.getState().currentView).toBe("agents");
  });

  it("keeps ideation domain state when the root changes", () => {
    useIdeationStore.getState().addSession({
      id: "session-1",
      projectId: "demo-project",
      title: "Test Session",
      status: "active",
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    });
    useIdeationStore.getState().setActiveSession("session-1");

    useUiStore.getState().setCurrentView("agents");

    expect(useIdeationStore.getState().activeSessionId).toBe("session-1");
  });
});
